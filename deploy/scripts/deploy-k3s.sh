#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs
# SPDX-License-Identifier: Apache-2.0
# Build (or pull) the relay-pubsub image and deploy it to a local k3s cluster
# via the Helm chart, using the self-contained memory backend so the test
# doesn't depend on a real Relay service or the relay-pubsub-secrets secret.
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
NAMESPACE="${NAMESPACE:-relay-pubsub}"
BUILDER="${BUILDER:-docker}"
PULL_REGISTRY="${PULL_REGISTRY:-}"
IMAGE_TAG="${IMAGE_TAG:-latest}"
SKIP_K3S_INSTALL="${SKIP_K3S_INSTALL:-}"

cd "${ROOT}"

if [ -z "${SKIP_K3S_INSTALL}" ]; then
    bash "${ROOT}/deploy/scripts/install-k3s-ubuntu.sh"
fi
export KUBECONFIG="${KUBECONFIG:-${HOME}/.kube/config}"

if [ -n "${PULL_REGISTRY}" ]; then
    IMAGE_REPO="${PULL_REGISTRY}/relay-pubsub"
    echo "Pulling ${IMAGE_REPO}:${IMAGE_TAG} into k3s containerd..."
    sudo k3s ctr -n k8s.io images pull "${IMAGE_REPO}:${IMAGE_TAG}"
else
    # docker save/import round-trips through the canonical docker.io/library/*
    # reference, so use that same ref for the Helm image.repository value below.
    IMAGE_REPO="docker.io/library/relay-pubsub"
    echo "Building ${IMAGE_REPO}:${IMAGE_TAG} locally with ${BUILDER}..."
    "${BUILDER}" build -t "${IMAGE_REPO}:${IMAGE_TAG}" -f Dockerfile .
    TARBALL="$(mktemp -t relay-pubsub-image.XXXXXX.tar)"
    "${BUILDER}" save "${IMAGE_REPO}:${IMAGE_TAG}" -o "${TARBALL}"
    sudo k3s ctr -n k8s.io images import "${TARBALL}"
    rm -f "${TARBALL}"
fi

echo "Helm upgrade --install..."
helm upgrade --install relay-pubsub "${ROOT}/deploy/helm/relay-pubsub" \
    -n "${NAMESPACE}" --create-namespace \
    --set image.repository="${IMAGE_REPO}" \
    --set image.tag="${IMAGE_TAG}" \
    --set image.pullPolicy=IfNotPresent \
    --set relay.backend=memory

echo "Waiting for rollout..."
kubectl -n "${NAMESPACE}" rollout status deployment/relay-pubsub --timeout=180s
kubectl -n "${NAMESPACE}" get pods -o wide

echo ""
echo "Deployed. To access locally:"
echo "  kubectl -n ${NAMESPACE} port-forward svc/relay-pubsub 8080:8080 &"
echo "  BASE=https://127.0.0.1:8080 bash scripts/smoke.sh"
