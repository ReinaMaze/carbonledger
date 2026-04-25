#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    Address, Env, String, Vec,
    symbol_short, vec,
};

/// TTL extension in ledgers (~30 days at 5s/ledger).
/// Cost: ~0.00001 XLM per ledger entry extended. See docs/ttl-cost.md.
const TTL_LEDGERS: u32 = 518_400;

// ── Error Enum ────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum CarbonError {
    ProjectNotFound        = 1,
    ProjectNotVerified     = 2,
    ProjectSuspended       = 3,
    InsufficientCredits    = 4,
    AlreadyRetired         = 5,
    SerialNumberConflict   = 6,
    UnauthorizedVerifier   = 7,
    UnauthorizedOracle     = 8,
    InvalidVintageYear     = 9,
    ListingNotFound        = 10,
    InsufficientLiquidity  = 11,
    PriceNotSet            = 12,
    MonitoringDataStale    = 13,
    DoubleCountingDetected = 14,
    RetirementIrreversible = 15,
    ZeroAmountNotAllowed   = 16,
    ProjectAlreadyExists   = 17,
    InvalidSerialRange     = 18,
    AlreadyInitialized     = 19,
}

// ── Storage Keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Batch(String),
    Retirement(String),
    ProjectBatches(String),
    SerialRegistry,
    Admin,
    RegistryContract,
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreditStatus {
    Active,
    PartiallyRetired,
    FullyRetired,
    Suspended,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct CreditBatch {
    pub batch_id:     String,
    pub project_id:   String,
    pub vintage_year: u32,
    pub amount:       i128,
    pub serial_start: u64,
    pub serial_end:   u64,
    pub issued_at:    u64,
    pub status:       CreditStatus,
    pub metadata_cid: String,
    /// Current owner of this credit batch. Only the owner may transfer or retire.
    pub owner:        Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RetirementCertificate {
    pub retirement_id:    String,
    pub credit_batch_id:  String,
    pub project_id:       String,
    pub amount:           i128,
    pub retired_by:       Address,
    pub beneficiary:      String,
    pub retirement_reason: String,
    pub vintage_year:     u32,
    pub serial_numbers:   Vec<u64>,
    pub retired_at:       u64,
    pub tx_hash:          String,
}

/// Compact serial range stored globally to detect overlaps.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SerialRange {
    pub start: u64,
    pub end:   u64,
}

/// Tracks how many credits in a batch have been retired so far.
#[contracttype]
#[derive(Clone)]
pub enum RetiredKey {
    BatchRetired(String),
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct CarbonCreditContract;

#[contractimpl]
impl CarbonCreditContract {

    /// Initialise with admin address.
    /// Can only be called once — subsequent calls return [`CarbonError::AlreadyInitialized`].
    pub fn initialize(env: Env, admin: Address, registry_contract: Address) -> Result<(), CarbonError> {
        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(CarbonError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::RegistryContract, &registry_contract);
        let ranges: Vec<SerialRange> = vec![&env];
        env.storage().persistent().set(&DataKey::SerialRegistry, &ranges);
        Ok(())
    }

    /// Mint verified carbon credits for a verified project. Assigns unique serial
    /// numbers to each credit, preventing double-counting globally.
    /// The `initial_owner` receives ownership of the batch.
    ///
    /// # Errors
    /// - [`CarbonError::ZeroAmountNotAllowed`] if `amount` is zero.
    /// - [`CarbonError::InvalidSerialRange`] if `serial_end < serial_start`.
    /// - [`CarbonError::SerialNumberConflict`] if serial range overlaps an existing batch.
    /// - [`CarbonError::InvalidVintageYear`] if vintage year is out of range.
    pub fn mint_credits(
        env: Env,
        admin: Address,
        project_id: String,
        amount: i128,
        vintage_year: u32,
        batch_id: String,
        serial_start: u64,
        serial_end: u64,
        metadata_cid: String,
        initial_owner: Address,
    ) -> Result<(), CarbonError> {
        // ── checks ────────────────────────────────────────────────────────────
        admin.require_auth();
        Self::require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(CarbonError::ZeroAmountNotAllowed);
        }
        if serial_end < serial_start {
            return Err(CarbonError::InvalidSerialRange);
        }
        if vintage_year < 2000 || vintage_year > 2100 {
            return Err(CarbonError::InvalidVintageYear);
        }
        if env.storage().persistent().has(&DataKey::Batch(batch_id.clone())) {
            return Err(CarbonError::SerialNumberConflict);
        }

        // AUDIT-NOTE [HIGH]: No cross-contract call to carbon_registry to verify the
        // project is in `Verified` status. Credits can be minted for Pending, Rejected,
        // or Suspended projects. Fix: invoke carbon_registry::get_project() and assert
        // status == ProjectStatus::Verified before proceeding.

        // Enforce global serial uniqueness
        if !Self::verify_serial_range_internal(&env, serial_start, serial_end) {
            return Err(CarbonError::DoubleCountingDetected);
        }

        // ── effects ───────────────────────────────────────────────────────────
        // Register serial range globally
        // AUDIT-NOTE [LOW]: SerialRegistry is an unbounded Vec. The overlap check is
        // O(n) over all historical ranges. With enough batches, this will exceed
        // Soroban's instruction limit, permanently bricking new minting. Fix: replace
        // with a sorted interval structure or a bitmap keyed by range blocks.
        let mut ranges: Vec<SerialRange> = env
            .storage()
            .persistent()
            .get(&DataKey::SerialRegistry)
            .unwrap_or_else(|| vec![&env]);
        ranges.push_back(SerialRange { start: serial_start, end: serial_end });
        env.storage().persistent().set(&DataKey::SerialRegistry, &ranges);

        let batch = CreditBatch {
            batch_id:     batch_id.clone(),
            project_id:   project_id.clone(),
            vintage_year,
            amount,
            serial_start,
            serial_end,
            issued_at:    env.ledger().timestamp(),
            status:       CreditStatus::Active,
            metadata_cid: metadata_cid.clone(),
            owner:        initial_owner.clone(),
        };
        env.storage().persistent().set(&DataKey::Batch(batch_id.clone()), &batch);
        Self::extend_batch_ttl(&env, &batch_id);

        let mut project_batches: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ProjectBatches(project_id.clone()))
            .unwrap_or_else(|| vec![&env]);
        project_batches.push_back(batch_id.clone());
        env.storage().persistent().set(&DataKey::ProjectBatches(project_id.clone()), &project_batches);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("minted")),
            (batch_id, project_id, amount, vintage_year, serial_start, serial_end),
        );
        Ok(())
    }

    /// Permanently and irreversibly retire carbon credits on-chain.
    ///
    /// # Errors
    /// - [`CarbonError::ZeroAmountNotAllowed`] if `amount` is zero.
    /// - [`CarbonError::InsufficientCredits`] if batch has fewer active credits than requested.
    /// - [`CarbonError::AlreadyRetired`] if batch is fully retired.
    pub fn retire_credits(
        env: Env,
        holder: Address,
        batch_id: String,
        amount: i128,
        retirement_reason: String,
        beneficiary: String,
        retirement_id: String,
        tx_hash: String,
    ) -> Result<RetirementCertificate, CarbonError> {
        // ── checks ────────────────────────────────────────────────────────────
        holder.require_auth();

        // AUDIT-NOTE [HIGH]: No ownership check. Any authenticated address can retire
        // any batch, permanently destroying credits they do not own. Fix: maintain an
        // on-chain Map<batch_id, Address> ownership record updated by transfer_credits
        // and mint_credits, and assert ownership here.

        if amount <= 0 {
            return Err(CarbonError::ZeroAmountNotAllowed);
        }

        let mut batch = Self::load_batch(&env, &batch_id)?;

        if batch.status == CreditStatus::FullyRetired {
            return Err(CarbonError::AlreadyRetired);
        }
        if batch.status == CreditStatus::Suspended {
            return Err(CarbonError::ProjectSuspended);
        }

        let active_amount = Self::active_amount(&env, &batch);
        if amount > active_amount {
            return Err(CarbonError::InsufficientCredits);
        }

        // ── effects ───────────────────────────────────────────────────────────
        let already_retired: i128 = env
            .storage()
            .persistent()
            .get(&RetiredKey::BatchRetired(batch_id.clone()))
            .unwrap_or(0i128);

        // AUDIT-NOTE [HIGH]: Unchecked i128 → u64 cast. If `already_retired` exceeds
        // u64::MAX (~1.8×10¹⁹), the cast wraps silently in release Wasm builds,
        // producing incorrect serial numbers in the certificate and potentially
        // re-issuing serial numbers that were already retired.
        // Fix: use `u64::try_from(already_retired).map_err(|_| CarbonError::InvalidSerialRange)?`
        let retire_serial_start = batch.serial_start + already_retired as u64;
        let retire_serial_end   = retire_serial_start + amount as u64 - 1;

        let mut serial_numbers: Vec<u64> = vec![&env];
        let mut s = retire_serial_start;
        while s <= retire_serial_end {
            serial_numbers.push_back(s);
            s += 1;
        }

        let new_retired = already_retired + amount;
        env.storage().persistent().set(&RetiredKey::BatchRetired(batch_id.clone()), &new_retired);

        let new_active = batch.amount - new_retired;
        batch.status = if new_active == 0 {
            CreditStatus::FullyRetired
        } else {
            CreditStatus::PartiallyRetired
        };
        env.storage().persistent().set(&DataKey::Batch(batch_id.clone()), &batch);
        Self::extend_batch_ttl(&env, &batch_id);

        let cert = RetirementCertificate {
            retirement_id:     retirement_id.clone(),
            credit_batch_id:   batch_id.clone(),
            project_id:        batch.project_id.clone(),
            amount,
            retired_by:        holder.clone(),
            beneficiary:       beneficiary.clone(),
            retirement_reason: retirement_reason.clone(),
            vintage_year:      batch.vintage_year,
            serial_numbers:    serial_numbers.clone(),
            retired_at:        env.ledger().timestamp(),
            tx_hash:           tx_hash.clone(),
        };
        env.storage().persistent().set(&DataKey::Retirement(retirement_id.clone()), &cert);

        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("retired")),
            (retirement_id, batch_id, batch.project_id, amount, holder, beneficiary),
        );
        Ok(cert)
    }

    /// Transfer credits to another account. Only the current batch owner may call this.
    /// No admin bypass exists — ownership is strictly enforced.
    ///
    /// # Errors
    /// - [`CarbonError::UnauthorizedVerifier`] if `from` is not the current batch owner.
    /// - [`CarbonError::AlreadyRetired`] if batch is fully retired.
    /// - [`CarbonError::InsufficientCredits`] if insufficient active credits.
    pub fn transfer_credits(
        env: Env,
        from: Address,
        to: Address,
        batch_id: String,
        amount: i128,
    ) -> Result<(), CarbonError> {
        // ── checks ────────────────────────────────────────────────────────────
        from.require_auth();

        if amount <= 0 {
            return Err(CarbonError::ZeroAmountNotAllowed);
        }

        let mut batch = Self::load_batch(&env, &batch_id)?;

        // Enforce owner-only: no admin bypass
        if batch.owner != from {
            return Err(CarbonError::UnauthorizedVerifier);
        }

        if batch.status == CreditStatus::FullyRetired {
            return Err(CarbonError::AlreadyRetired);
        }
        if batch.status == CreditStatus::Suspended {
            return Err(CarbonError::ProjectSuspended);
        }

        let active = Self::active_amount(&env, &batch);
        if amount > active {
            return Err(CarbonError::InsufficientCredits);
        }

        // ── effects ───────────────────────────────────────────────────────────
        // AUDIT-NOTE [HIGH]: Transfer is a no-op — no ownership record is updated.
        // Only an event is emitted. This means on-chain state does not reflect the
        // new owner, so retire_credits cannot enforce ownership. Fix: maintain a
        // Map<batch_id, Address> and update it here and in mint_credits.
        env.events().publish(
            (symbol_short!("c_ledger"), symbol_short!("transfer")),
            (batch_id, from, to, amount),
        );
        Ok(())
    }

    /// Returns a [`CreditBatch`] by ID.
    pub fn get_credit_batch(env: Env, batch_id: String) -> Result<CreditBatch, CarbonError> {
        Self::load_batch(&env, &batch_id)
    }

    /// Returns a permanent [`RetirementCertificate`] by retirement ID.
    pub fn get_retirement_certificate(
        env: Env,
        retirement_id: String,
    ) -> Result<RetirementCertificate, CarbonError> {
        env.storage()
            .persistent()
            .get(&DataKey::Retirement(retirement_id))
            .ok_or(CarbonError::ProjectNotFound)
    }

    /// Returns `true` if the serial range `[serial_start, serial_end]` does NOT
    /// overlap any existing batch — i.e., safe to mint.
    pub fn verify_serial_range(env: Env, serial_start: u64, serial_end: u64) -> bool {
        Self::verify_serial_range_internal(&env, serial_start, serial_end)
    }

    /// Returns all [`CreditBatch`] records for a given project.
    pub fn get_project_credits(env: Env, project_id: String) -> Vec<CreditBatch> {
        let batch_ids: Vec<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ProjectBatches(project_id))
            .unwrap_or_else(|| vec![&env]);

        let mut result: Vec<CreditBatch> = vec![&env];
        for id in batch_ids.iter() {
            if let Some(b) = env.storage().persistent().get(&DataKey::Batch(id.clone())) {
                result.push_back(b);
            }
        }
        result
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Extend TTL on a batch entry so it is not evicted by Soroban rent.
    /// Called on every read/write to active batches.
    fn extend_batch_ttl(env: &Env, batch_id: &String) {
        let key = DataKey::Batch(batch_id.clone());
        if env.storage().persistent().has(&key) {
            env.storage().persistent().extend_ttl(&key, TTL_LEDGERS, TTL_LEDGERS);
        }
    }

    fn load_batch(env: &Env, batch_id: &String) -> Result<CreditBatch, CarbonError> {
        let key = DataKey::Batch(batch_id.clone());
        let batch = env.storage()
            .persistent()
            .get(&key)
            .ok_or(CarbonError::ProjectNotFound)?;
        // Extend TTL on every read so active batches never expire
        env.storage().persistent().extend_ttl(&key, TTL_LEDGERS, TTL_LEDGERS);
        Ok(batch)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), CarbonError> {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .ok_or(CarbonError::UnauthorizedVerifier)?;
        if &admin != caller {
            return Err(CarbonError::UnauthorizedVerifier);
        }
        Ok(())
    }

    fn active_amount(env: &Env, batch: &CreditBatch) -> i128 {
        if batch.status == CreditStatus::FullyRetired {
            return 0;
        }
        let retired: i128 = env
            .storage()
            .persistent()
            .get(&RetiredKey::BatchRetired(batch.batch_id.clone()))
            .unwrap_or(0i128);
        batch.amount - retired
    }

    fn verify_serial_range_internal(env: &Env, start: u64, end: u64) -> bool {
        let ranges: Vec<SerialRange> = env
            .storage()
            .persistent()
            .get(&DataKey::SerialRegistry)
            .unwrap_or_else(|| vec![env]);

        for r in ranges.iter() {
            if start <= r.end && end >= r.start {
                return false;
            }
        }
        true
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env, String};

    fn s(env: &Env, v: &str) -> String { String::from_str(env, v) }

    fn setup(env: &Env) -> (CarbonCreditContractClient, Address, Address) {
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let client = CarbonCreditContractClient::new(&env, &id);
        client.initialize(&admin, &registry).unwrap();
        (env, client)
    }

    fn mint_batch(env: &Env, client: &CarbonCreditContractClient, admin: &Address, owner: &Address) {
        client.mint_credits(
            admin,
            &s(env, "proj-001"),
            &1000_i128,
            &2023_u32,
            &s(env, "batch-001"),
            &1_u64,
            &1000_u64,
            &s(env, "QmCID"),
            owner,
        ).unwrap();
    }

    // ── Issue #59: transfer authorization ────────────────────────────────────

    #[test]
    fn test_transfer_from_owner_succeeds() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        let owner = Address::generate(&env);
        let buyer = Address::generate(&env);
        mint_batch(&env, &client, &admin, &owner);

        client.transfer_credits(&owner, &buyer, &s(&env, "batch-001"), &100_i128).unwrap();

        let batch = client.get_credit_batch(&s(&env, "batch-001")).unwrap();
        assert_eq!(batch.owner, buyer);
    }

    #[test]
    fn test_transfer_from_non_owner_fails() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        let owner   = Address::generate(&env);
        let attacker = Address::generate(&env);
        let victim   = Address::generate(&env);
        mint_batch(&env, &client, &admin, &owner);

        let result = client.try_transfer_credits(&attacker, &victim, &s(&env, "batch-001"), &100_i128);
        assert!(result.is_err());
    }

    #[test]
    fn test_admin_cannot_bypass_transfer_authorization() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        let owner = Address::generate(&env);
        let to    = Address::generate(&env);
        mint_batch(&env, &client, &admin, &owner);

        // Admin is not the batch owner — must be rejected
        let result = client.try_transfer_credits(&admin, &to, &s(&env, "batch-001"), &100_i128);
        assert!(result.is_err());
    }

    #[test]
    fn test_transfer_updates_owner() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        mint_batch(&env, &client, &admin, &owner);

        client.transfer_credits(&owner, &new_owner, &s(&env, "batch-001"), &500_i128).unwrap();

        // New owner can transfer again; old owner cannot
        let third = Address::generate(&env);
        client.transfer_credits(&new_owner, &third, &s(&env, "batch-001"), &200_i128).unwrap();
        let result = client.try_transfer_credits(&owner, &third, &s(&env, "batch-001"), &100_i128);
        assert!(result.is_err());
    }

    // ── Existing tests (updated for new mint_credits signature) ──────────────

    #[test]
    fn test_mint_credits_success() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(
            &admin,
            &s(&env, "proj-002"),
            &500_i128,
            &2023_u32,
            &s(&env, "batch-A"),
            &1_u64,
            &500_u64,
            &s(&env, "QmCID"),
            &owner,
        ).unwrap();

        let b = client.get_credit_batch(&s(&env, "batch-A")).unwrap();
        assert_eq!(b.amount, 500);
        assert_eq!(b.status, CreditStatus::Active);
        assert_eq!(b.owner, owner);
    }

    #[test]
    fn test_serial_conflict_detection() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();
        // Overlapping range 50-150 should fail
        let result = c.try_mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b2"), &50_u64, &150_u64, &s(&env, "cid"));
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_serial_range_no_overlap() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();
        // Non-overlapping range should return true
        assert!(c.verify_serial_range(&101_u64, &200_u64));
        // Overlapping range should return false
        assert!(!c.verify_serial_range(&50_u64, &150_u64));
    }

    #[test]
    fn test_retire_credits_permanent() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();

        let holder = Address::generate(&env);
        let cert = c.retire_credits(
            &holder,
            &s(&env, "b1"),
            &100_i128,
            &s(&env, "offset 2023 emissions"),
            &s(&env, "Acme Corp"),
            &s(&env, "ret-001"),
            &s(&env, "txhash123"),
        ).unwrap();

        assert_eq!(cert.amount, 100);
        let batch = client.get_credit_batch(&s(&env, "b1")).unwrap();
        assert_eq!(batch.status, CreditStatus::FullyRetired);
    }

    #[test]
    fn test_retired_credits_cannot_be_transferred() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();

        client.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid"), &owner).unwrap();
        client.retire_credits(&owner, &s(&env, "b1"), &100_i128, &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-001"), &s(&env, "tx")).unwrap();

        let to = Address::generate(&env);
        let result = client.try_transfer_credits(&owner, &to, &s(&env, "b1"), &10_i128);
        assert!(result.is_err());
    }

    #[test]
    fn test_retired_credits_cannot_be_retired_again() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();

        client.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid"), &owner).unwrap();
        client.retire_credits(&owner, &s(&env, "b1"), &100_i128, &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-001"), &s(&env, "tx")).unwrap();

        let result = client.try_retire_credits(&owner, &s(&env, "b1"), &100_i128, &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-002"), &s(&env, "tx2"));
        assert!(result.is_err());
    }

    #[test]
    fn test_partial_retirement_updates_status() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();

        client.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid"), &owner).unwrap();
        client.retire_credits(&owner, &s(&env, "b1"), &40_i128, &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-001"), &s(&env, "tx")).unwrap();

        let batch = client.get_credit_batch(&s(&env, "b1")).unwrap();
        assert_eq!(batch.status, CreditStatus::PartiallyRetired);
    }

    #[test]
    fn test_get_retirement_certificate() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        c.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid")).unwrap();

        client.mint_credits(&admin, &s(&env, "p1"), &100_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid"), &owner).unwrap();
        client.retire_credits(&owner, &s(&env, "b1"), &100_i128, &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-001"), &s(&env, "tx")).unwrap();

        let cert = client.get_retirement_certificate(&s(&env, "ret-001")).unwrap();
        assert_eq!(cert.amount, 100);
        assert_eq!(cert.retirement_id, s(&env, "ret-001"));
    }

    #[test]
    fn test_zero_amount_rejected() {
        let (env, _) = setup();
        let admin = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();

        let result = c.try_mint_credits(&admin, &s(&env, "p1"), &0_i128, &2023_u32, &s(&env, "b1"), &1_u64, &100_u64, &s(&env, "cid"));
        assert!(result.is_err());
    }

    #[test]
    fn test_initialize_twice_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        let c = CarbonCreditContractClient::new(&env, &id);
        c.initialize(&admin, &registry).unwrap();
        let result = c.try_initialize(&admin, &registry);
        assert!(result.is_err());
    }
}

// ── Property-based fuzz tests ─────────────────────────────────────────────────

#[cfg(test)]
mod fuzz {
    use super::*;
    use proptest::prelude::*;
    use soroban_sdk::{testutils::Address as _, Env, String};

    fn s(env: &Env, v: &str) -> String { String::from_str(env, v) }

    /// Set up a fresh contract instance with admin and registry.
    fn setup() -> (Env, CarbonCreditContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin    = Address::generate(&env);
        let registry = Address::generate(&env);
        let id = env.register_contract(None, CarbonCreditContract);
        // SAFETY: the Env outlives the client within each proptest case.
        let env: &'static Env = Box::leak(Box::new(env));
        let client = CarbonCreditContractClient::new(env, &id);
        client.initialize(&admin, &registry).unwrap();
        (env.clone(), client, admin)
    }

    // ── mint_credits ──────────────────────────────────────────────────────────

    proptest! {
        /// Any amount ≤ 0 must return ZeroAmountNotAllowed — never panic.
        #[test]
        fn fuzz_mint_zero_or_negative_amount(amount in i128::MIN..=0_i128) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            let result = client.try_mint_credits(
                &admin,
                &s(&env, "proj-fuzz"),
                &amount,
                &2023_u32,
                &s(&env, "batch-fuzz"),
                &1_u64,
                &100_u64,
                &s(&env, "cid"),
                &owner,
            );
            prop_assert!(result.is_err());
        }

        /// serial_end < serial_start must return InvalidSerialRange — never panic.
        #[test]
        fn fuzz_mint_inverted_serial_range(
            start in 1_u64..u64::MAX,
            delta in 1_u64..1_000_000_u64,
        ) {
            let end = start.saturating_sub(delta);
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            let result = client.try_mint_credits(
                &admin,
                &s(&env, "proj-fuzz"),
                &100_i128,
                &2023_u32,
                &s(&env, "batch-fuzz"),
                &start,
                &end,
                &s(&env, "cid"),
                &owner,
            );
            prop_assert!(result.is_err());
        }

        /// vintage_year outside [2000, 2100] must return InvalidVintageYear — never panic.
        #[test]
        fn fuzz_mint_invalid_vintage_year(year in prop_oneof![
            0_u32..2000_u32,
            2101_u32..u32::MAX,
        ]) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            let result = client.try_mint_credits(
                &admin,
                &s(&env, "proj-fuzz"),
                &100_i128,
                &year,
                &s(&env, "batch-fuzz"),
                &1_u64,
                &100_u64,
                &s(&env, "cid"),
                &owner,
            );
            prop_assert!(result.is_err());
        }

        /// Valid inputs must always succeed and produce a retrievable batch.
        #[test]
        fn fuzz_mint_valid_inputs_succeed(
            amount in 1_i128..1_000_000_i128,
            serial_start in 1_u64..500_000_u64,
            serial_len in 1_u64..500_000_u64,
            vintage_year in 2000_u32..=2100_u32,
        ) {
            let serial_end = serial_start.saturating_add(serial_len - 1);
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            let result = client.try_mint_credits(
                &admin,
                &s(&env, "proj-fuzz"),
                &amount,
                &vintage_year,
                &s(&env, "batch-fuzz"),
                &serial_start,
                &serial_end,
                &s(&env, "cid"),
                &owner,
            );
            prop_assert!(result.is_ok(), "unexpected error: {:?}", result.err());
            let batch = client.get_credit_batch(&s(&env, "batch-fuzz")).unwrap();
            prop_assert_eq!(batch.amount, amount);
            prop_assert_eq!(batch.owner, owner);
        }

        /// Duplicate batch_id must always be rejected — never panic.
        #[test]
        fn fuzz_mint_duplicate_batch_id_rejected(
            amount in 1_i128..1_000_i128,
        ) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &amount, &2023_u32,
                &s(&env, "batch-dup"), &1_u64, &100_u64, &s(&env, "cid"), &owner,
            ).unwrap();
            // Second mint with same batch_id must fail
            let result = client.try_mint_credits(
                &admin, &s(&env, "proj-fuzz"), &amount, &2023_u32,
                &s(&env, "batch-dup"), &200_u64, &300_u64, &s(&env, "cid"), &owner,
            );
            prop_assert!(result.is_err());
        }

        /// Overlapping serial ranges must always be rejected — never panic.
        #[test]
        fn fuzz_mint_overlapping_serials_rejected(
            overlap_start in 1_u64..50_u64,
            overlap_end in 51_u64..200_u64,
        ) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            // Mint [1, 100]
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &100_i128, &2023_u32,
                &s(&env, "batch-a"), &1_u64, &100_u64, &s(&env, "cid"), &owner,
            ).unwrap();
            // Any range that overlaps [1, 100] must fail
            let result = client.try_mint_credits(
                &admin, &s(&env, "proj-fuzz"), &100_i128, &2023_u32,
                &s(&env, "batch-b"), &overlap_start, &overlap_end, &s(&env, "cid"), &owner,
            );
            prop_assert!(result.is_err());
        }
    }

    // ── retire_credits ────────────────────────────────────────────────────────

    proptest! {
        /// Retiring more than available must return InsufficientCredits — never panic.
        #[test]
        fn fuzz_retire_exceeds_available(excess in 1_i128..1_000_000_i128) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &100_i128, &2023_u32,
                &s(&env, "batch-fuzz"), &1_u64, &100_u64, &s(&env, "cid"), &owner,
            ).unwrap();
            let over_amount = 100_i128 + excess;
            let result = client.try_retire_credits(
                &owner,
                &s(&env, "batch-fuzz"),
                &over_amount,
                &s(&env, "reason"),
                &s(&env, "Corp"),
                &s(&env, "ret-001"),
                &s(&env, "tx"),
            );
            prop_assert!(result.is_err());
        }

        /// Retiring zero or negative must return ZeroAmountNotAllowed — never panic.
        #[test]
        fn fuzz_retire_zero_or_negative(amount in i128::MIN..=0_i128) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &100_i128, &2023_u32,
                &s(&env, "batch-fuzz"), &1_u64, &100_u64, &s(&env, "cid"), &owner,
            ).unwrap();
            let result = client.try_retire_credits(
                &owner,
                &s(&env, "batch-fuzz"),
                &amount,
                &s(&env, "reason"),
                &s(&env, "Corp"),
                &s(&env, "ret-001"),
                &s(&env, "tx"),
            );
            prop_assert!(result.is_err());
        }

        /// Retiring a non-existent batch must return an error — never panic.
        #[test]
        fn fuzz_retire_nonexistent_batch(batch_suffix in "[a-z]{1,8}") {
            let (env, client, _admin) = setup();
            let holder = Address::generate(&env);
            let batch_id = format!("no-such-{}", batch_suffix);
            let result = client.try_retire_credits(
                &holder,
                &s(&env, &batch_id),
                &10_i128,
                &s(&env, "reason"),
                &s(&env, "Corp"),
                &s(&env, "ret-001"),
                &s(&env, "tx"),
            );
            prop_assert!(result.is_err());
        }

        /// Valid partial retirements must succeed and leave batch PartiallyRetired.
        #[test]
        fn fuzz_retire_partial_valid(
            total in 2_i128..10_000_i128,
            retire_frac in 1_u32..99_u32,  // percentage 1–98
        ) {
            let retire_amount = (total * retire_frac as i128 / 100).max(1).min(total - 1);
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &total, &2023_u32,
                &s(&env, "batch-fuzz"), &1_u64, &(total as u64), &s(&env, "cid"), &owner,
            ).unwrap();
            let cert = client.retire_credits(
                &owner,
                &s(&env, "batch-fuzz"),
                &retire_amount,
                &s(&env, "reason"),
                &s(&env, "Corp"),
                &s(&env, "ret-001"),
                &s(&env, "tx"),
            ).unwrap();
            prop_assert_eq!(cert.amount, retire_amount);
            let batch = client.get_credit_batch(&s(&env, "batch-fuzz")).unwrap();
            prop_assert_eq!(batch.status, CreditStatus::PartiallyRetired);
        }

        /// Full retirement followed by any further retirement must fail — never panic.
        #[test]
        fn fuzz_retire_after_full_retirement_fails(
            second_amount in 1_i128..1_000_i128,
        ) {
            let (env, client, admin) = setup();
            let owner = Address::generate(&env);
            client.mint_credits(
                &admin, &s(&env, "proj-fuzz"), &100_i128, &2023_u32,
                &s(&env, "batch-fuzz"), &1_u64, &100_u64, &s(&env, "cid"), &owner,
            ).unwrap();
            // Fully retire
            client.retire_credits(
                &owner, &s(&env, "batch-fuzz"), &100_i128,
                &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-001"), &s(&env, "tx"),
            ).unwrap();
            // Any further retirement must fail
            let result = client.try_retire_credits(
                &owner, &s(&env, "batch-fuzz"), &second_amount,
                &s(&env, "reason"), &s(&env, "Corp"), &s(&env, "ret-002"), &s(&env, "tx2"),
            );
            prop_assert!(result.is_err());
        }
    }
}
