# Pass this file to terraform init:
#   terraform init -backend-config=backend.hcl
#
# Replace <ACCOUNT_ID> with your AWS account ID after running the bootstrap module.

bucket         = "carbonledger-terraform-state-<ACCOUNT_ID>"
key            = "carbonledger/terraform.tfstate"
region         = "us-east-1"
dynamodb_table = "carbonledger-terraform-lock"
encrypt        = true
