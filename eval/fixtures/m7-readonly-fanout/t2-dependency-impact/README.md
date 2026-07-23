# Dependency impact

`affected_components(changed)` reads every JSON document in `components/`.
Return all components transitively affected by the changed component, including
the changed component itself, in sorted order. A component is affected when it
depends directly or indirectly on an affected component.

Do not hard-code the fixture values.
