#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: Apache-2.0
# Idempotent k3s + kubectl + helm bootstrap for Ubuntu/Debian hosts.
# Safe to re-run.
set -euo pipefail

K3S_BIN="${K3S_BIN:-/usr/local/bin/k3s}"
KUBECONFIG_PATH="${KUBECONFIG_PATH:-${HOME}/.kube/config}"

if ! command -v k3s &>/dev/null; then
    echo "Installing k3s..."
    curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="--write-kubeconfig-mode 644" sh -
else
    echo "k3s already installed: $(k3s --version | head -1)"
fi

mkdir -p "$(dirname "${KUBECONFIG_PATH}")"
if [ -f /etc/rancher/k3s/k3s.yaml ]; then
    sudo cp /etc/rancher/k3s/k3s.yaml "${KUBECONFIG_PATH}"
    sudo chown "$(id -u)":"$(id -g)" "${KUBECONFIG_PATH}"
    chmod 600 "${KUBECONFIG_PATH}"
fi
export KUBECONFIG="${KUBECONFIG_PATH}"

if ! command -v kubectl &>/dev/null && [ -x "${K3S_BIN}" ]; then
    sudo ln -sf "${K3S_BIN}" /usr/local/bin/kubectl 2>/dev/null || true
fi

if ! command -v helm &>/dev/null; then
    echo "Installing helm..."
    curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash
else
    echo "helm already installed: $(helm version --short 2>/dev/null || true)"
fi

echo "Waiting for k3s node to be Ready..."
for _ in $(seq 1 60); do
    kubectl get nodes 2>/dev/null | grep -q ' Ready' && break
    sleep 2
done
kubectl get nodes -o wide
