#!/usr/bin/env bash
# Build, publish to the local server folder, then copy the firmware to the
# Google Cloud VM that the devices talk to.
#   tools/publish-vm.sh
set -euo pipefail
cd "$(dirname "$0")/.."
DIR="${HOOT_SERVER_DIR:-$HOME/Code/Hardware/frame-server}"
VM="${HOOT_VM:-frames}"
ZONE="${HOOT_VM_ZONE:-us-central1-a}"
PROJECT="${HOOT_GCP_PROJECT:-$(cat /tmp/hoot-gcp-project 2>/dev/null || true)}"
tools/release.sh "$DIR"
gcloud compute scp --quiet --zone "$ZONE" ${PROJECT:+--project "$PROJECT"} \
  "$DIR/firmware/hoot.bin" "$DIR/firmware/version.txt" "$VM:~/frame-server/firmware/"
echo "on the VM: $(curl -s http://hoot.arpanpandey.dev/firmware/version.txt)"
