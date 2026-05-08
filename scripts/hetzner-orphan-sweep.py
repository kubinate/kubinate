#!/usr/bin/env python3
# Sprint 1 ticket 08 — nightly E2E force-destroy backstop.
#
# Reads JSON output of `GET /v1/servers?label_selector=...` from
# stdin and DELETEs every server it finds. Used by
# `.github/workflows/nightly-e2e.yml` as the safety net so a
# crashed harness can never leave the test cluster lying around
# accruing cost.
#
# Reads `HCLOUD_TOKEN` from the env (the workflow passes it via
# `env:`, not `${{ }}` interpolation, per the GitHub workflow-
# injection guidance).
#
# Lives in `scripts/` rather than embedded in the workflow YAML
# because GitHub's YAML validator stricter-parses workflow files
# and chokes on heredoc-quoted Python with escape characters.

import json
import os
import sys
import urllib.request


def main() -> int:
    token = os.environ.get("HCLOUD_TOKEN")
    if not token:
        print("HCLOUD_TOKEN not set; refusing to run", file=sys.stderr)
        return 1
    try:
        data = json.load(sys.stdin)
    except json.JSONDecodeError as exc:
        print(f"failed to parse stdin as JSON: {exc}", file=sys.stderr)
        return 1
    servers = data.get("servers", [])
    if not servers:
        print("no orphan servers found")
        return 0
    failures = 0
    for srv in servers:
        srv_id = srv.get("id")
        srv_name = srv.get("name", "<unknown>")
        if srv_id is None:
            print(f"  skipping server without id: {srv}", file=sys.stderr)
            continue
        print(f"force-deleting orphan {srv_id} ({srv_name})")
        req = urllib.request.Request(
            f"https://api.hetzner.cloud/v1/servers/{srv_id}",
            method="DELETE",
            headers={"Authorization": f"Bearer {token}"},
        )
        try:
            urllib.request.urlopen(req).read()
        except Exception as exc:
            print(f"  delete failed: {exc}", file=sys.stderr)
            failures += 1
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
