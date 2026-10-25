variable "IMAGE_TAG" {
  default = "local"
}

variable "CI_COMMIT_SHA" {
  default = IMAGE_TAG
}

variable "IMAGE_REPOSITORY" {
  default = "agentlink-integration"
}

# Optional CI publication defaults are supplied by an additional Bake file.
target "_publish" {}

group "default" {
  targets = ["integration"]
}

target "integration" {
  inherits = ["_publish"]
  context = "."
  dockerfile = "tests/integration/Dockerfile"
  tags = ["${IMAGE_REPOSITORY}:${CI_COMMIT_SHA}"]
}
