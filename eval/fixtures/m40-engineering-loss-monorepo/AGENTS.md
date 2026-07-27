# Evaluation repository rules

- Every top-level project is independent. Work only in the project named by the task.
- Do not modify `_eval_verifier.py`, tests, fixtures, logs, lockfiles, or generated outputs unless the task explicitly includes that path.
- Preserve public function signatures and existing build identities.
- Use the project verifier command before proposing completion.
- Do not infer missing source-of-truth data or weaken a test to make it pass.

Project verifier commands:

- Rust projects: `/usr/bin/python3 -I -B <project>/_eval_verifier.py <project>`
- Go project: `/usr/bin/python3 -I -B go-cli-exit/_eval_verifier.py go-cli-exit`
- Python projects: `/usr/bin/python3 -I -B <project>/_eval_verifier.py <project>`
- TypeScript project: `/usr/bin/python3 -I -B typescript-utf8-frames/_eval_verifier.py typescript-utf8-frames`
