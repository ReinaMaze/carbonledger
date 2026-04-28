#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    Address, Env, String, Vec,
    symbol_short, vec, BytesN,
};

// ── Error Enum ────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CarbonError {
    ProjectNotFound       = 1,
    ProjectNotVerified    = 2,
    ProjectSuspended      = 3,
    InsufficientCredits   = 4,
    AlreadyRetired        = 5,
    SerialNumberConflict  = 6,
    UnauthorizedVerifier  = 7,
    UnauthorizedOracle    = 8,
    InvalidVintageYear    = 9,
    ListingNotFound       = 10,
    InsufficientLiquidity = 11,
    PriceNotSet           = 12,
    MonitoringDataStale   = 13,
    DoubleCountingDetected = 14,
    RetirementIrreversible = 15,
    ZeroAmountNotAllowed  = 16,
    ProjectAlreadyExists  = 17,
    InvalidSerialRange    = 18,
    AlreadyInitialized    = 19,
    MethodologyScoreLow   = 20,
    UnauthorizedUpgrade   = 21,
}

// ── Storage Keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Project(String),
    Verifiers,
    OracleAddress,
    RegistryAdmin,
    ContractVersion,
    UpgradeHistory,
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectStatus {
    Pending,
    Verified,
    Rejected,
    Suspended,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CarbonProject {
    pub project_id:            String,
    pub name:                  String,
    pub methodology:           String,
    pub country:               String,
    pub project_type:          String,
    pub verifier_address:      Address,
    pub metadata_cid:          String,
    pub total_credits_issued:  i128,
    pub total_credits_retired: i128,
    pub methodology_score:     u32,
    pub status:                ProjectStatus,
    pub vintage_year:          u32,
    pub created_at:            u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct UpgradeRecord {
    pub from_version: u32,
    pub to_version:   u32,
    pub timestamp:    u64,
    pub upgraded_by:  Address,
    pub wasm_hash:    BytesN<32>,
}

// ── Contract ──────────────────────────────────────────────────────────────────

const CURRENT_VERSION: u32 = 1;

#[contract]
pub struct CarbonRegistryContract;

#[contractimpl]
impl CarbonRegistryContract {

    pub fn initialize(
        env: Env,
        admin: Address,
        oracle_address: Address,
        verifiers: Vec<Address>,
    ) -> Result<(), CarbonError> {
        if env.storage().persistent().has(&DataKey::RegistryAdmin) {
            return Err(CarbonError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().persistent().set(&DataKey::RegistryAdmin, &admin);
        env.storage().persistent().set(&DataKey::OracleAddress, &oracle_address);
        env.storage().persistent().set(&DataKey::Verifiers, &verifiers);
        env.storage().persistent().set(&DataKey::ContractVersion, &CURRENT_VERSION);
        Ok(())
    }

    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let current_version: u32 = env.storage()
            .persistent()
            .get(&DataKey::ContractVersion)
            .unwrap_or(1);

        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());

        let next_version = current_version + 1;
        env.storage().persistent().set(&DataKey::ContractVersion, &next_version);

        let record = UpgradeRecord {
            from_version: current_version,
            to_version:   next_version,
            timestamp:    env.ledger().timestamp(),
            upgraded_by:  admin.clone(),
            wasm_hash:    new_wasm_hash,
        };
        env.storage().persistent().set(&DataKey::UpgradeHistory, &record);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("upgraded")),
            (current_version, next_version, admin),
        );
        Ok(())
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ContractVersion)
            .unwrap_or(1)
    }

    pub fn get_upgrade_history(env: Env) -> Option<UpgradeRecord> {
        env.storage()
            .persistent()
            .get(&DataKey::UpgradeHistory)
    }

    fn current_year(env: &Env) -> u32 {
        let seconds_per_year: u64 = 31557600;
        let timestamp = env.ledger().timestamp();
        1970 + (timestamp / seconds_per_year) as u32
    }

    pub fn register_project(
        env: Env,
        admin: Address,
        project_id: String,
        name: String,
        metadata_cid: String,
        verifier_address: Address,
        methodology: String,
        country: String,
        project_type: String,
        methodology_score: u32,
        vintage_year: u32,
    ) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if project_id.is_empty() || project_id.chars().count() > 64 {
            return Err(CarbonError::ProjectNotFound);
        }
        if name.is_empty() || name.chars().count() > 128 {
            return Err(CarbonError::ProjectNotFound);
        }
        if metadata_cid.is_empty() || metadata_cid.chars().count() > 128 {
            return Err(CarbonError::ProjectNotFound);
        }
        if methodology.is_empty() || methodology.chars().count() > 64 {
            return Err(CarbonError::ProjectNotFound);
        }
        if country.is_empty() || country.chars().count() > 64 {
            return Err(CarbonError::ProjectNotFound);
        }
        if project_type.is_empty() || project_type.chars().count() > 64 {
            return Err(CarbonError::ProjectNotFound);
        }

        let current_year = Self::current_year(&env);
        if vintage_year < 1990 || vintage_year > current_year + 1 {
            return Err(CarbonError::InvalidVintageYear);
        }

        if methodology_score < 70 {
            return Err(CarbonError::MethodologyScoreLow);
        }

        if env.storage().persistent().has(&DataKey::Project(project_id.clone())) {
            return Err(CarbonError::ProjectAlreadyExists);
        }

        let project = CarbonProject {
            project_id:            project_id.clone(),
            name:                  name.clone(),
            methodology:           methodology.clone(),
            country:               country.clone(),
            project_type:          project_type.clone(),
            verifier_address:      verifier_address.clone(),
            metadata_cid:          metadata_cid.clone(),
            total_credits_issued:  0,
            total_credits_retired: 0,
            methodology_score,
            status:                ProjectStatus::Pending,
            vintage_year,
            created_at:            env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("reg_proj")),
            (project_id, methodology, country, vintage_year, methodology_score),
        );
        Ok(())
    }

    pub fn verify_project(
        env: Env,
        verifier_address: Address,
        project_id: String,
    ) -> Result<(), CarbonError> {
        verifier_address.require_auth();
        Self::require_verifier(&env, &verifier_address)?;

        let mut project = Self::load_project(&env, &project_id)?;
        project.status = ProjectStatus::Verified;
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("verified")),
            (project_id, verifier_address),
        );
        Ok(())
    }

    pub fn reject_project(
        env: Env,
        verifier_address: Address,
        project_id: String,
        reason: String,
    ) -> Result<(), CarbonError> {
        verifier_address.require_auth();
        Self::require_verifier(&env, &verifier_address)?;

        let mut project = Self::load_project(&env, &project_id)?;
        project.status = ProjectStatus::Rejected;
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("rejected")),
            (project_id, verifier_address, reason),
        );
        Ok(())
    }

    pub fn update_project_status(
        env: Env,
        oracle_address: Address,
        project_id: String,
        status: ProjectStatus,
    ) -> Result<(), CarbonError> {
        oracle_address.require_auth();
        Self::require_oracle(&env, &oracle_address)?;

        let mut project = Self::load_project(&env, &project_id)?;
        project.status = status.clone();
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("st_update")),
            (project_id, oracle_address),
        );
        Ok(())
    }

    pub fn suspend_project(
        env: Env,
        admin: Address,
        project_id: String,
        reason: String,
    ) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let mut project = Self::load_project(&env, &project_id)?;
        project.status = ProjectStatus::Suspended;
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("suspended")),
            (project_id, admin, reason),
        );
        Ok(())
    }

    pub fn get_project(env: Env, project_id: String) -> Result<CarbonProject, CarbonError> {
        Self::load_project(&env, &project_id)
    }

    pub fn increment_issued(
        env: Env,
        oracle_address: Address,
        project_id: String,
        amount: i128,
    ) -> Result<(), CarbonError> {
        oracle_address.require_auth();
        Self::require_oracle(&env, &oracle_address)?;
        let mut project = Self::load_project(&env, &project_id)?;
        project.total_credits_issued = project.total_credits_issued.checked_add(amount).ok_or(CarbonError::Arithmetic)?;
        env.storage().persistent().set(&DataKey::Project(project_id), &project);
        Ok(())
    }

    pub fn retire_credits(
        env: Env,
        admin: Address,
        project_id: String,
        amount: i128,
    ) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(CarbonError::ZeroAmountNotAllowed);
        }

        let mut project = Self::load_project(&env, &project_id)?;

        if project.total_credits_retired + amount > project.total_credits_issued {
            return Err(CarbonError::InsufficientCredits);
        }

        project.total_credits_retired = project.total_credits_retired.checked_add(amount).ok_or(CarbonError::Arithmetic)?;
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &project);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("retired")),
            (project_id, amount),
        );
        Ok(())
    }

    pub fn add_verifier(env: Env, admin: Address, verifier: Address) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let verifiers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Verifiers)
            .unwrap_or_else(|| vec![&env]);

        if !verifiers.contains(&verifier) {
            let new_verifiers = verifiers.push_back(verifier);
            env.storage().persistent().set(&DataKey::Verifiers, &new_verifiers);
        }

        Ok(())
    }

    pub fn remove_verifier(env: Env, admin: Address, verifier: Address) -> Result<(), CarbonError> {
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        let verifiers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Verifiers)
            .unwrap_or_else(|| vec![&env]);

        if let Some(index) = verifiers.first_index_of(&verifier) {
            let new_verifiers = verifiers.remove(index);
            env.storage().persistent().set(&DataKey::Verifiers, &new_verifiers);
        }

        Ok(())
    }

    pub fn get_verifiers(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::Verifiers)
            .unwrap_or_else(|| vec![&env])
    }

    fn load_project(env: &Env, project_id: &String) -> Result<CarbonProject, CarbonError> {
        env.storage()
            .persistent()
            .get(&DataKey::Project(project_id.clone()))
            .ok_or(CarbonError::ProjectNotFound)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), CarbonError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::RegistryAdmin)
            .ok_or(CarbonError::UnauthorizedVerifier)?;
        if &admin != caller {
            return Err(CarbonError::UnauthorizedVerifier);
        }
        Ok(())
    }

    fn require_verifier(env: &Env, caller: &Address) -> Result<(), CarbonError> {
        let verifiers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Verifiers)
            .unwrap_or_else(|| vec![env]);
        if !verifiers.contains(caller) {
            return Err(CarbonError::UnauthorizedVerifier);
        }
        Ok(())
    }

    fn require_oracle(env: &Env, caller: &Address) -> Result<(), CarbonError> {
        let oracle: Address = env
            .storage()
            .persistent()
            .get(&DataKey::OracleAddress)
            .ok_or(CarbonError::UnauthorizedOracle)?;
        if &oracle != caller {
            return Err(CarbonError::UnauthorizedOracle);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::{Address as _}, vec, Env, String};

    fn setup() -> (Env, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let oracle   = Address::generate(&env);
        let verifier = Address::generate(&env);
        let client = CarbonRegistryContractClient::new(&env, &env.register_contract(None, CarbonRegistryContract));
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);
        (env, admin, oracle, verifier)
    }

    fn make_str(env: &Env, s: &str) -> String {
        String::from_str(env, s)
    }

    fn register(env: &Env, client: &CarbonRegistryContractClient, admin: &Address) {
        client.register_project(
            admin,
            &make_str(env, "proj-001"),
            &make_str(env, "Amazon Reforestation"),
            &make_str(env, "QmCID123"),
            &Address::generate(env),
            &make_str(env, "VCS"),
            &make_str(env, "Brazil"),
            &make_str(env, "forestry"),
            &75_u32,
            &2023_u32,
        );
    }

    #[test]
    fn test_register_project_valid() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.status, ProjectStatus::Pending);
        assert_eq!(p.vintage_year, 2023);
    }

    #[test]
    fn test_register_duplicate_fails() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        let result = client.try_register_project(
            &admin,
            &make_str(&env, "proj-001"),
            &make_str(&env, "Dup"),
            &make_str(&env, "cid"),
            &Address::generate(&env),
            &make_str(&env, "VCS"),
            &make_str(&env, "Brazil"),
            &make_str(&env, "forestry"),
            &75_u32,
            &2023_u32,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_verifier_approves_project() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.verify_project(&verifier, &make_str(&env, "proj-001"));
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.status, ProjectStatus::Verified);
    }

    #[test]
    fn test_unauthorized_verifier_rejected() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        let rogue = Address::generate(&env);
        let result = client.try_verify_project(&rogue, &make_str(&env, "proj-001"));
        assert!(result.is_err());
    }

    #[test]
    fn test_verifier_rejects_project() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.reject_project(&verifier, &make_str(&env, "proj-001"), &make_str(&env, "fraud"));
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.status, ProjectStatus::Rejected);
    }

    #[test]
    fn test_oracle_updates_status() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.update_project_status(&oracle, &make_str(&env, "proj-001"), &ProjectStatus::Completed);
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.status, ProjectStatus::Completed);
    }

    #[test]
    fn test_admin_suspends_project() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.suspend_project(&admin, &make_str(&env, "proj-001"), &make_str(&env, "investigation"));
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.status, ProjectStatus::Suspended);
    }

    #[test]
    fn test_get_project_returns_correct_data() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        let p = client.get_project(&make_str(&env, "proj-001"));
        assert_eq!(p.project_id, make_str(&env, "proj-001"));
        assert_eq!(p.country, make_str(&env, "Brazil"));
        assert_eq!(p.total_credits_issued, 0);
    }

    #[test]
    fn test_register_score_too_low_fails() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]).unwrap();

        let result = client.try_register_project(
            &admin,
            &make_str(&env, "proj-low"),
            &make_str(&env, "Low Score"),
            &make_str(&env, "cid"),
            &Address::generate(&env),
            &make_str(&env, "VCS"),
            &make_str(&env, "Brazil"),
            &make_str(&env, "forestry"),
            &69_u32,
            &2023_u32,
        );
        assert_eq!(result, Err(Ok(CarbonError::MethodologyScoreLow)));
    }

    #[test]
    fn test_register_score_minimum_succeeds() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]).unwrap();

        client.register_project(
            &admin,
            &make_str(&env, "proj-min"),
            &make_str(&env, "Min Score"),
            &make_str(&env, "cid"),
            &Address::generate(&env),
            &make_str(&env, "VCS"),
            &make_str(&env, "Brazil"),
            &make_str(&env, "forestry"),
            &70_u32,
            &2023_u32,
        ).unwrap();

        let p = client.get_project(&make_str(&env, "proj-min")).unwrap();
        assert_eq!(p.methodology_score, 70);
    }

    #[test]
    fn test_initialize_twice_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let oracle   = Address::generate(&env);
        let verifier = Address::generate(&env);
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);
        let result = client.try_initialize(&admin, &oracle, &vec![&env, verifier.clone()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_overflow_increment_issued_graceful_error() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.increment_issued(&oracle, &make_str(&env, "proj-001"), &10_i128);
        
        let result = client.try_increment_issued(&oracle, &make_str(&env, "proj-001"), &i128::MAX);
        assert_eq!(result.unwrap_err().unwrap(), CarbonError::Arithmetic);
    }

    #[test]
    fn test_retire_credits_success() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.verify_project(&verifier, &make_str(&env, "proj-001")).unwrap();
        client.increment_issued(&oracle, &make_str(&env, "proj-001"), &1000_i128).unwrap();
        client.retire_credits(&admin, &make_str(&env, "proj-001"), &300_i128).unwrap();

        let p = client.get_project(&make_str(&env, "proj-001")).unwrap();
        assert_eq!(p.total_credits_retired, 300);
    }

    #[test]
    fn test_retire_credits_cannot_exceed_issued() {
        let (env, admin, oracle, verifier) = setup();
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]);

        register(&env, &client, &admin);
        client.verify_project(&verifier, &make_str(&env, "proj-001")).unwrap();
        client.increment_issued(&oracle, &make_str(&env, "proj-001"), &100_i128).unwrap();

        let result = client.try_retire_credits(&admin, &make_str(&env, "proj-001"), &200_i128);
        assert_eq!(result.unwrap_err().unwrap(), CarbonError::InsufficientCredits);
    }

    #[test]
    fn test_upgrade_admin_only() {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let oracle   = Address::generate(&env);
        let verifier = Address::generate(&env);
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]).unwrap();

        let attacker = Address::generate(&env);
        let fake_hash = BytesN::from_array(&env, &[0u8; 32]);
        let result = client.try_upgrade(&attacker, &fake_hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_version_tracking() {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let oracle   = Address::generate(&env);
        let verifier = Address::generate(&env);
        let contract_id = env.register_contract(None, CarbonRegistryContract);
        let client = CarbonRegistryContractClient::new(&env, &contract_id);
        client.initialize(&admin, &oracle, &vec![&env, verifier.clone()]).unwrap();

        assert_eq!(client.get_version(), 1);
    }
}
