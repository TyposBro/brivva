terraform {
  required_version = ">= 1.6"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  backend "s3" {
    bucket         = "brivva-tf-state"
    key            = "brivva/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "brivva-tf-locks"
  }
}

provider "aws" {
  region = var.region
}
