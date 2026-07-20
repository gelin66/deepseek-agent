# Range coalescing task

`coalesce_ranges` converts integers into sorted inclusive ranges. The current
implementation mishandles the first value after a gap.

Run the public test with:

```bash
python3 -m unittest -q
```
