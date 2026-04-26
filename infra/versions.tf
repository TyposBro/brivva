terraform {
  required_version = ">= 1.6"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  # S3 backend is provisioned (brivva-tf-state + brivva-tf-locks in us-east-1)
  # but NOT enabled yet. Committed terraform.tfstate was stale relative to
  # real infrastructure — enabling the backend without first reconciling drift
  # would cause `tofu apply` to try to recreate ~12 resources that already
  # exist in AWS. See docs/terraform-state-migration-plan.md for the
  # drift-resolution procedure.
  # backend "s3" {
  #   bucket         = "brivva-tf-state"
  #   key            = "brivva/terraform.tfstate"
  #   region         = "us-east-1"
  #   encrypt        = true
  #   dynamodb_table = "brivva-tf-locks"
  # }
}

provider "aws" {
  region = var.region
}
