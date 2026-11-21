# ADR-009 — `legacy/` congelado durante la migración

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0–F5

## Contexto

Necesitamos el binario C como **oráculo de paridad** (compara salidas, códigos de error y árboles),
pero el objetivo final es eliminar todo el C del producto. Si el C se modifica durante la migración,
el oráculo deja de ser estable.

## Decisión

- Todo el C (fuentes, headers, makefile, `build*.cmd`, CI histórico, `flake.nix`) se mueve a
  `legacy/` en F0, sin cambios funcionales.
- `legacy/` se compila solo bajo demanda (`scripts/build-legacy.ps1|sh`) y solo se usa en el job
  `parity` de CI y en el desarrollo local.
- Prohibido añadir features al C; solo fixes críticos de seguridad, replicados en Rust.
- En F5 se borra `legacy/` y queda archivado en el tag `v2.6-c-final`.
- El workflow `cmake-multi-platform.yml` se elimina en F0: apuntaba a un `CMakeLists.txt` inexistente
  y su sitio lo ocupa `ci.yml`.

## Consecuencias

- `git ls-files '*.c'` deja de ser ruido: todo lo que quede en `crates/` es Rust.
- Los merges de `upstream` (`lusid1/pkg2zip`) se congelan: se registra el commit base en
  `docs/paridad.md` y se resolverán después de 3.0.0.
- El binario C no se publica en releases.

## Alternativas descartadas

- **Dejar el C en la raíz:** mezcla responsabilidades y complica la verificación "cero C".
- **Copiar el C a otra rama y borrarlo de `develop`:** perderíamos el oráculo durante la migración.
