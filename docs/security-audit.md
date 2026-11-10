# Security audit map

Each row is a threat the agent runtime already refuses, and the test that fails if that refusal changes. These tests run under `cargo test --locked --workspace`. They are part of the normal check. They are not a separate release gate.

| Threat | Test |
| --- | --- |
| A router names a remote or wildcard endpoint | `remote_and_wildcard_endpoints_are_rejected` |
| The default router loads model weights | `default_router_does_not_name_a_model` |
| Secret-shaped text is stored in the clear | `test_secret_detection` |
| A secret-named dirty file enters context | `secret_named_dirty_file_is_omitted_from_context` |
| Context keeps a secret or ignores the token budget | `context_redacts_secrets_and_obeys_a_tight_budget` |
| Config load follows a symlink | `test_config_refuses_symlink_and_does_not_follow_it` |
| The daemon socket is not private to the user | `test_socket_bind_and_permissions` |
| The data directory is not mode `0700` | `test_private_dir_is_mode_0700` |
| A symlink is indexed | `symlinks_are_not_indexed` |
| `status.json` is group-readable or omits the phase | `status_file_is_private_and_reports_the_phase_line` |
| A GNOME decision is stored without the displayed args hash | `gnome_decision_requires_the_displayed_hash_and_forces_gnome_source` |
| MCP approves its own call | `mcp_cannot_approve_itself` |
| An MCP approver is stored when the hash matches | `mcp_approver_is_rejected_even_when_the_hash_matches` |
| Desktop tools run without consent or despite the kill file | `calls_are_denied_without_consent_or_when_the_kill_file_exists` |
| A tool call without consent is reported as success | `tool_call_without_consent_is_an_error_result_and_list_is_local` |
| Typed text is echoed back to the caller | `a_consented_fake_tree_lists_clicks_and_does_not_echo_typed_text` |
| A live accessibility bus is treated as a usable tree | `session_probe_errors_without_a_bus_and_refuses_a_reachable_bus` |
| Evidence is accepted with an empty contract, a bad digest, or an applied ask | `ingest_requires_a_matching_digest_and_rejects_an_applied_ask` |
| Opening a schema-5 catalog takes a write lock | `current_catalog_opens_while_a_writer_holds_the_reserved_lock` |
| A skill path escapes the root, names an unknown tool, or follows a symlink | `rejects_path_escape_unknown_tools_and_symlinks` |
| A capability token is recoverable from its stored form | `token_is_stored_as_a_hash_and_compared_without_the_secret` |
| `02 dashboard` binds something other than `127.0.0.1` | `dashboard_binary_listener_is_loopback` |
| The dashboard renders a loose or symlinked status file | `dashboard_page_reads_the_private_status_file` |

Desktop automation still has no shell, no network client, and no screenshot upload. Consent is the whitespace word `allow` in a mode `0600` file. The kill file wins when both exist.

agt checks the same `args_hash` the runtime displays. Approval does not recompute it. Those checks live in the agt repository, in the policy, evidence, and audit packages.
