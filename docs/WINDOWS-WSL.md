# Windows and WSL/Linux

Primary development environment Windows11 Enterprise, Node24.21.0/npm12.0.2/PowerShell7. Windows
Server2025 Datacenter support is a target, not an installation claim. Hosted Windows CI is labeled
as such, not as testing the owner's exact OS. Local portable dependencies remain inside the project.

From any CWD use absolute scripts/setup.ps1; it resolves its source root. Install/Update uses locked
npm ci/build/smoke, stops on command failures, never silently installs/moves runtimes. Diagnose is
provider-free. Register/Remove require explicit absolute ClientConfig and only owned entry changes,
with backups, an interprocess lock and concurrent-change detection. -WhatIf validates intent without
build/smoke/host mutation. Safe TOML editing supports bare single-line tables; quoted/array/duplicate/
multiline/inline MCP forms are refused for manual entry-scoped editing. A stale lock is not stolen.
Backups allow manual rollback; no automatic rollback action overwrites a changed config.
No service, startup entry, task, extension or firewall change. No persistent execution-policy change.

Linux: npm ci/build, configure absolute Linux roots and launch Node directly. Persistent WSL source
copy convention /srv/projects/numera-image-gen-mcp; Windows G: examples are not Linux paths.
Windows files may be /mnt/g/... only when mount exists. The process running MCP determines path
semantics. localhost may differ between WSL/Windows depending on networking mode; configure reachable
API origin explicitly, never replace the host model. No runner installation or automatic GPU tools.

ComfyUI owns GPU device selection. Numera checks reported primary device; no guessed per-request
CUDA switch. No real workflow/checkpoint/device benchmark here without configured authorized backend.
