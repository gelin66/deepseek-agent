# Nested settings merge task

`merge_settings` returns a recursively merged configuration without mutating
either input. The current implementation replaces a complete nested mapping
when only one nested value is overridden.

Run the public test with:

```bash
python3 -m unittest -q
```
