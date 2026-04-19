# Terraform state → S3 backend migration plan

Status: **planned, NOT started**.
Blocker: executing while `deploy.yml` is running risks state corruption.

## Current state (bad)

`infra/versions.tf` has no `backend` block, so Terraform uses local
state. `infra/terraform.tfstate` + `terraform.tfstate.backup` are
committed to git.

Problems this creates:
- State contains resource IDs, secret ARNs, account ID. Leaks on a repo
  share or a fork.
- Two people can't run `terraform apply` without conflict — last write
  wins, earlier work silently lost.
- Merge conflicts on state JSON are unrecoverable without manual
  resource tree reconstruction.
- No state locking — concurrent CI runs can corrupt state.

## Target state (good)

S3 backend with DynamoDB state lock:

```hcl
terraform {
  required_version = ">= 1.6"
  backend "s3" {
    bucket         = "brivva-tf-state"
    key            = "brivva/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "brivva-tf-locks"
  }
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}
```

## Migration steps (run when no deploy is in-flight)

1. **Pre-create S3 bucket + DynamoDB table manually** (chicken-and-egg:
   backend storage can't be managed by the same state that uses it):

   ```bash
   # bucket
   aws s3api create-bucket \
     --bucket brivva-tf-state \
     --region us-east-1
   aws s3api put-bucket-versioning \
     --bucket brivva-tf-state \
     --versioning-configuration Status=Enabled
   aws s3api put-bucket-encryption \
     --bucket brivva-tf-state \
     --server-side-encryption-configuration \
     '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}'
   aws s3api put-public-access-block \
     --bucket brivva-tf-state \
     --public-access-block-configuration \
     BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true

   # lock table
   aws dynamodb create-table \
     --table-name brivva-tf-locks \
     --attribute-definitions AttributeName=LockID,AttributeType=S \
     --key-schema AttributeName=LockID,KeyType=HASH \
     --billing-mode PAY_PER_REQUEST \
     --region us-east-1
   ```

2. **Add backend block to `versions.tf`** (edit shown above).

3. **Run `terraform init -migrate-state`**. Terraform detects local
   state, prompts to push to S3. Accept.

4. **Verify** — `terraform plan` should show zero changes. If it wants
   to recreate resources, something migrated wrong — rollback to local
   state from backup and investigate.

5. **Remove committed state files from git history**:

   ```bash
   git rm infra/terraform.tfstate infra/terraform.tfstate.backup
   git commit -m "infra: remove local state, migrated to S3 backend"
   ```

6. **Rewrite history to purge committed secrets** (optional but
   recommended — state contains secret ARNs):

   ```bash
   # CAUTION: rewrites history. Coordinate with anyone holding a clone.
   git filter-repo --path infra/terraform.tfstate --invert-paths
   git filter-repo --path infra/terraform.tfstate.backup --invert-paths
   # force-push to remote after coordinating
   ```

7. **Update `.gitignore`**:

   ```
   infra/.terraform/
   infra/terraform.tfstate
   infra/terraform.tfstate.backup
   infra/*.tfvars  # should already be ignored
   ```

8. **Update CI** (`deploy.yml`) — no change needed if it already
   runs `terraform init` before plan/apply. S3 backend is
   auto-discovered.

## When to do it

- **NOT during active deploy.** Deploy agent runs `terraform apply`
  against local state. Migration mid-flight = lost work.
- **Window:** right after a successful deploy lands + before the next
  one. Typically off-hours or during a deliberate infra pause.
- **Before May 10 launch.** Launch week is not the time for state
  surgery.

Recommended: schedule for a weekend after May 10 stabilizes, or a
low-activity evening with Simon informed.

## Rollback if migration fails

1. Remove the `backend "s3"` block from `versions.tf`.
2. Copy `terraform.tfstate` back from the S3 bucket to `infra/`.
3. `terraform init -reconfigure` to reset to local backend.

Keep the S3 bucket version history for 30 days before deleting.

## Non-goals

- Multi-workspace support (dev/staging/prod). Brivva is single-env
  right now. Add workspaces only when staging appears.
- Importing existing resources. All current resources are already in
  state, migration preserves that.
- CI-based state surgery. Humans only, one at a time, with a clear
  head.
