# ADR-005 — Escritor ZIP/ZIP64 propio

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F2

## Contexto

El ZIP no es un detalle: su estructura es parte del contrato (Vita/Adrenaline esperan ese layout).
Además el C necesita cosas que los crates de ZIP no ofrecen:

1. escribir un archivo y **fijar su CRC después** (el CRC del CSO se conoce tras escribir la
   cabecera y el índice, que van al principio);
2. escribir en un **offset arbitrario** dentro de una entrada (`zip_write_file_at`);
3. reajustar el offset de escritura tras reservar la cabecera CSO (`zip_set_offset`);
4. control exacto de atributos DOS y del orden de entradas.

## Decisión

Portar `pkg2zip_zip.c` a un `ZipWriter` con la misma máquina de estados:

```text
begin(root) / add_folder / add_parent / begin_file / write / write_at / set_offset / end_file / close
```

Soporta ZIP64 (tamaños > 4 GiB, offsets > 4 GiB, > 65.535 entradas) igual que el C, con los mismos
`version needed` (45), flag UTF-8 y atributos.

## Consecuencias

- Bytes equivalentes a los del C (salvo fecha/hora, que se fija en tests).
- Se puede validar con `unzip -t`, `7z t` y `zipfile` de Python.
- Coste: ~450 líneas propias y tests con archivos > 4 GiB (dispersos) en CI nocturno.

## Alternativas descartadas

- **Crate `zip`:** no permite escribir en offsets ni fijar CRC a posteriori; además impone su propio
  orden de entradas y metadatos.
- **Escribir el ZIP en memoria:** los PKG pueden superar 4 GiB.
