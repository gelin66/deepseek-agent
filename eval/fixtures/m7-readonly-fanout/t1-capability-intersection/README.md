# Capability intersection

`compatibility()` must read every JSON document in `specs/` and return:

- the highest required `minimum`;
- the sorted feature intersection shared by every component;
- the sorted component names.

Do not hard-code the fixture values.
