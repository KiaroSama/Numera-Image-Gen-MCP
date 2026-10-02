# Test results

Status2026-10-02: implementation in progress; final CI matrix/coverage not yet run.

| Check                                 | Observed result                                                | Environment                                  |
| ------------------------------------- | -------------------------------------------------------------- | -------------------------------------------- |
| Foundation URL/redaction seam         | 13 passed                                                      | local Node24.21.0 Windows11 Enterprise       |
| Adapter mapping seam                  | 9 passed                                                       | local fixture contracts, no real provider    |
| Built MCP two-process seam            | 1 passed;10 tools;one POST;exact output bytes                  | real SDK stdio clients + loopback provider   |
| Provider-free smoke                   | handshake/tools/health passed                                  | built executable                             |
| Strict typecheck/build/lint           | passed on latest inspected source before final docs/test edits | local toolchain                              |
| Runtime npm audit                     | 0 vulnerabilities at initial check                             | locked runtime metadata                      |
| Final Windows/Linux offline matrix    | not_run yet                                                    | pending final integrated CI                  |
| Coverage85/80 gates                   | unmeasured                                                     | pending final CI                             |
| Packed clean install/setup merge      | not_run yet                                                    | pending final verification                   |
| Paid/live gateway/provider generation | not_run                                                        | no endpoint/model/count/cost authorization   |
| Actual Claude/Codex GUI               | not_run                                                        | no host registration/mutation authorized     |
| ComfyUI real GPU/checkpoint workflow  | not_run                                                        | no approved installed workflow/model/backend |
| Windows Server2025 installation       | not_run                                                        | no such test host                            |

No universal compatibility, production-readiness or model-quality percentage claimed.
