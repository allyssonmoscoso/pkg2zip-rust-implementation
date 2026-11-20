# ADR-010 — Versión 3.0.0 y compatibilidad de CLI

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F5

## Contexto

Una reescritura completa justifica un cambio de major, pero los usuarios integran `pkg2zip` en
scripts (nombres de salida, `-l`, códigos de salida). Romper la CLI tendría coste real aunque el
formato de salida no cambie.

## Decisión

- La primera release Rust es **3.0.0**.
- Se conservan flags, orden de argumentos, nombres de archivo, estructura interna, mensajes de
  stdout/stderr y códigos `0`/`1`.
- Cambios permitidos (aditivos o de robustez): `--version`, mensajes de error con más contexto en
  stderr, validación de tamaños antes de desbordar, `-h` idéntico salvo el número de versión.
- Cualquier diferencia observable se documenta en `CHANGELOG.md` y en las notas del release.

## Consecuencias

- Cambiar el binario 2.6 → 3.0 no requiere tocar scripts.
- El arnés de paridad se vuelve un contrato ejecutable: si cambia un mensaje, falla CI.
- El tag `v2.6-c-final` conserva la implementación anterior para quien necesite reproducirla.

## Alternativas descartadas

- **Mantener el número 2.x:** ocultaría que es una reescritura y complicaría el soporte.
- **CLI nueva "limpia":** rompería scripts y los tests de paridad no tendrían sentido.
