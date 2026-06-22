# Oracle Test Deferrals

This inventory is for focused tests that look like stock-oracle coverage but
must not be imported into Git `2.47.1` behavior matrices yet.

Each entry keeps the generated oracle backlog honest: the test is reviewed, but
it is not a closed Git `2.47.1` row.

| Evidence | Classification | Reason |
| --- | --- | --- |
| `git_history_query_compat::whatchanged_requires_explicit_opt_in_like_git_2_54` | deferred | The test asserts Git `2.54` removal-warning behavior and does not compare stock Git `2.47.1` stdout, stderr, exit code and side effects. Keep it out of Git `2.47.1` matrices until a real `2.47.1` whatchanged oracle row is added. |
