# Optional V3 only. Nothing here is applied by the desktop or by a build.
terraform {
  required_version = ">= 1.6, < 2.0"
  required_providers {
    google = { source = "hashicorp/google", version = ">= 6.36, < 8.0" }
  }
}
provider "google" {
  project = var.project_id
  region  = var.region
}
variable "project_id" { type = string }
variable "region" {
  type    = string
  default = "us-central1"
}
variable "image" {
  type        = string
  description = "Previously built OCI image pinned by sha256 digest. This configuration does not build or publish images."
  validation {
    condition     = can(regex("@sha256:[0-9a-f]{64}$", var.image))
    error_message = "Use an immutable image digest."
  }
}
variable "principals_secret_id" {
  type        = string
  description = "Existing Secret Manager secret containing PCC_PRINCIPALS_JSON. Secret values are not placed in Terraform state."
}
variable "principals_secret_version" { type = string }
variable "license_policies" {
  type        = map(object({ plan = string, features = list(string), max_members = number, expires_at = number }))
  default     = {}
  description = "Optional hosted-only entitlements; empty means unrestricted self-hosting. Never gates local data or exports."
}
variable "source_hosts" {
  type    = string
  default = "export.arxiv.org,hnrss.org"
}
variable "enable_collector_schedule" {
  type    = bool
  default = false
}
resource "google_project_service" "apis" {
  for_each           = toset(["run.googleapis.com", "storage.googleapis.com", "cloudscheduler.googleapis.com", "secretmanager.googleapis.com", "iam.googleapis.com"])
  service            = each.value
  disable_on_destroy = false
}
resource "google_service_account" "host" {
  account_id   = "pcc-optional-host"
  display_name = "Optional PCC tenant state and collector service"
  depends_on   = [google_project_service.apis]
}
resource "google_service_account" "scheduler" {
  account_id   = "pcc-collector-scheduler"
  display_name = "PCC scheduled job invoker only"
  depends_on   = [google_project_service.apis]
}
resource "google_storage_bucket" "state" {
  name                        = "${var.project_id}-pcc-optional-state"
  location                    = var.region
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = false
  versioning { enabled = false }
  soft_delete_policy { retention_duration_seconds = 0 }
  depends_on = [google_project_service.apis]
}
resource "google_storage_bucket_iam_member" "state_access" {
  bucket = google_storage_bucket.state.name
  role   = "roles/storage.objectUser"
  member = "serviceAccount:${google_service_account.host.email}"
}
resource "google_secret_manager_secret_iam_member" "principals" {
  secret_id = var.principals_secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.host.email}"
}
resource "google_cloud_run_v2_service" "host" {
  name                = "pcc-optional-host"
  location            = var.region
  deletion_protection = true
  template {
    service_account                  = google_service_account.host.email
    timeout                          = "300s"
    max_instance_request_concurrency = 24
    scaling {
      min_instance_count = 0
      max_instance_count = 2
    }
    containers {
      image = var.image
      resources { limits = { cpu = "1", memory = "512Mi" } }
      env {
        name  = "PCC_STATE_BUCKET"
        value = google_storage_bucket.state.name
      }
      env {
        name  = "PCC_LICENSES_JSON"
        value = jsonencode(var.license_policies)
      }
      env {
        name  = "PCC_SOURCE_HOSTS"
        value = var.source_hosts
      }
      env {
        name = "PCC_PRINCIPALS_JSON"
        value_source {
          secret_key_ref {
            secret  = var.principals_secret_id
            version = var.principals_secret_version
          }
        }
      }
    }
  }
  depends_on = [google_storage_bucket_iam_member.state_access, google_secret_manager_secret_iam_member.principals]
}
# Native clients authenticate with independently revocable application access keys.
# The transport endpoint is public; all data endpoints enforce tenant auth in the app.
resource "google_cloud_run_v2_service_iam_member" "native_transport" {
  name     = google_cloud_run_v2_service.host.name
  location = var.region
  role     = "roles/run.invoker"
  member   = "allUsers"
}
resource "google_cloud_run_v2_job" "collector" {
  name     = "pcc-public-collectors"
  location = var.region
  template {
    template {
      service_account = google_service_account.host.email
      timeout         = "600s"
      max_retries     = 0
      containers {
        image   = var.image
        command = ["python", "worker.py"]
        resources { limits = { cpu = "1", memory = "512Mi" } }
        env {
          name  = "PCC_STATE_BUCKET"
          value = google_storage_bucket.state.name
        }
        env {
          name  = "PCC_LICENSES_JSON"
          value = jsonencode(var.license_policies)
        }
        env {
          name  = "PCC_SOURCE_HOSTS"
          value = var.source_hosts
        }
        env {
          name = "PCC_PRINCIPALS_JSON"
          value_source {
            secret_key_ref {
              secret  = var.principals_secret_id
              version = var.principals_secret_version
            }
          }
        }
      }
    }
  }
  depends_on = [google_storage_bucket_iam_member.state_access, google_secret_manager_secret_iam_member.principals]
}
resource "google_cloud_run_v2_job_iam_member" "scheduler_invoke" {
  name     = google_cloud_run_v2_job.collector.name
  location = var.region
  role     = "roles/run.invoker"
  member   = "serviceAccount:${google_service_account.scheduler.email}"
}
resource "google_cloud_scheduler_job" "collect" {
  count            = var.enable_collector_schedule ? 1 : 0
  name             = "pcc-collect-every-15-minutes"
  region           = var.region
  schedule         = "*/15 * * * *"
  time_zone        = "Etc/UTC"
  attempt_deadline = "180s"
  http_target {
    http_method = "POST"
    uri         = "https://run.googleapis.com/v2/projects/${var.project_id}/locations/${var.region}/jobs/${google_cloud_run_v2_job.collector.name}:run"
    oauth_token { service_account_email = google_service_account.scheduler.email }
  }
  depends_on = [google_cloud_run_v2_job_iam_member.scheduler_invoke]
}
output "host_origin" { value = google_cloud_run_v2_service.host.uri }
