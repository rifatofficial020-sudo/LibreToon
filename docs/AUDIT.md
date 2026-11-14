# Auditoría e importación de Animate — noviembre de 2026

Revisión estática de las rutas de documentos/recuperación, modelo y validación, composición y símbolos, escenas/interpolación, rigging/Puppet, biblioteca/portapapeles, geometría y pinceles, render/caché, texto/tableta y exportación. Se revisaron especialmente accesos a índices, ciclos, trabajo repetido, límites de memoria y tareas que bloqueaban la interfaz. Las pruebas de regresión y los renderizados sin ventana complementan esta revisión; no equivalen a una certificación de ausencia de errores ni a una sesión de edición en Windows con tableta.

## Compatibilidad de entrada FLA

| Contenido de Animate | Resultado |
| --- | --- |
| FLA ZIP/XFL CS5+; entradas almacenadas o Deflate; XML UTF‑8/UTF‑16 con BOM | Importación |
| Escenas, capas, claves, exposiciones y claves vacías | Conservadas, con el orden de dibujo/escenas |
| Símbolos gráficos anidados | Definiciones y tiempos conservados; referencias ausentes/recursivas omitidas con aviso |
| Movie clips | Reloj independiente de escena, incluso dentro de un gráfico de un fotograma |
| Botones | Estado Up estático; sin estados interactivos ni navegación |
| ActionScript/JavaScript y audio | Omitidos; nunca se ejecuta código |
| Grupos y matrices afines | Conservados |
| Colores, alfa de instancias y degradados de relleno | Conservados; alfa/multiplicadores RGBA interpolables |
| Interpolación clásica de movimiento compatible | Transformación y aceleración aproximada entre claves; no giros múltiples ni trayectoria exacta |
| Interpolación de forma, motion-object XML y topologías incompatibles | Claves y exposiciones; aviso de interpolación no reconstruida |
| Texto clásico/estático/dinámico | Contornos con fuentes locales; sustitución y disposición aproximadas, avisadas |
| TLF sin recorridos clásicos, imágenes, vídeo, componentes y bitmap fills | Omitidos con aviso |
| Carpetas/máscaras/guías de capas | Aplanadas o excluidas del dibujo según tipo; comportamiento de máscara no reconstruido |
| Efectos, tintes/brillo especiales y blend modes | Omitidos/aproximados con aviso |
| Fondo de escenario de color | Capa vectorial de fondo bloqueada |
| FPS fraccionarios | Redondeados con aviso; el modelo actual solo admite FPS enteros |
| FLA binario anterior a CS5, cifrado, ZIP64 o XFL descomprimido | No admitidos |

No se ha recibido una colección de archivos reales del usuario. Las regresiones construyen fixtures de los elementos XFL y comprueban geometría, colores y orden temporal, incluida una entrada Deflate. La reproducción puede diferir en proyectos cuyo comportamiento dependa de scripts. No se promete equivalencia visual de efectos o tipografía no implementados. Los contornos bajo escalado no tienen todos los modos de grosor de Animate.

## Hallazgos corregidos

| Prioridad | Hallazgo y consecuencia | Corrección / evidencia |
| --- | --- | --- |
| Alta | Se rechazaba un FLA entero por incluir un script, símbolo, grupo o sonido | Importador tolerante por elemento y reporte; pruebas de escenas/scripts/medios ausentes y botones |
| Alta | Guardar una importación parcial podía sobrescribir la fuente y perder elementos omitidos | Primera operación de guardar requiere elegir una copia; se mantiene el informe en el proyecto nativo |
| Alta | El caché `done` del DFS podía ocultar exceso de profundidad cuando un hijo ya se había validado por otra raíz | Profundidad máxima memoizada del DAG, independiente del orden de biblioteca; regresión de cadena invertida de nueve símbolos |
| Alta | Un DAG pequeño sin ciclos podía producir expansión exponencial de geometría | Presupuesto conservador de dos millones de puntos de control expandidos (máximos por capa/símbolo), antes del render; regresión con 30 instancias por nivel |
| Alta | Portapapeles entre documentos: mismo identificador numérico podía referirse a un símbolo diferente | Verificación de definiciones y dependencias; snapshot COW y vía rápida dentro del mismo documento |
| Media | Al pegar se reutilizaban identificadores de correspondencia de interpolación | Se asignan identificadores nuevos; los vínculos de rigging de las copias se remapean al nuevo identificador |
| Media | Apertura/validación/importación completa en el hilo de interfaz | Un lector en segundo plano, una petición pendiente reemplazable y un resultado; prueba de solicitudes repetidas y última petición |
| Media | Se volvían a recorrer bordes/puntos de una exposición larga como si todos sus fotogramas fueran geometría distinta | Validación por identidad COW de fotograma y grafo de símbolos con aristas deduplicadas |
| Media | Búsqueda repetida del directorio ZIP para cada recurso; cabeceras locales inconsistentes y duplicados ambiguos | Índice de directorio, comprobación de cabecera/CRC/tamaños y rechazo de nombres duplicados |
| Media | Entradas XML/proyectos y diagnósticos podían crecer demasiado antes de procesarse | Lectura de proyecto 256 MiB, entrada ZIP 64 MiB, biblioteca XML 128 MiB, límites de nodos/profundidad/entidades/atributos y avisos acotados |
| Media | Texto completo de stderr de FFmpeg retenido sin límite | Se drena la tubería y solo se conservan los últimos 64 KiB; prueba de drenaje de 300.000 bytes |
| Media | Consultar un fotograma fuera de rango o una capa vacía podía provocar un panic | Consulta defensiva y regresión de capa vacía/`usize::MAX` |
| Media | Un movie clip anidado podía avanzar antes de aparecer su instancia padre | El reloj del hijo descuenta la entrada del padre; regresión de aparición tardía en escena |
| Media | Un gesto de hueso/pin/pesos podía quedar activo al cambiar de herramienta, pestaña o deshacer | Se cierra o cancela el gesto en esas transiciones, sin reanudarlo al regresar |
| Baja | Pruebas podían leer/escribir la recuperación de inicio del entorno general y alterar otras pruebas | Recuperación de inicio aislada por proceso/hilo durante pruebas |

La importación no intenta reparar XML corrupto, cifrado ni contenedores inválidos. Omitir un elemento incompatible no autoriza ejecutar su contenido. No se extraen rutas ZIP a disco.

## Riesgos y trabajo pendiente

- Guardar FLA sigue siendo síncrono: generar su proyección XFL recorre y materializa fotogramas. Escenas largas o deformaciones complejas pueden bloquear durante el guardado y usar mucha RAM. El límite de 64 MiB del contenedor no es un límite global de memoria.
- Selección, bote, recortes y ciertos pinceles no tienen índice espacial. Las operaciones booleanas y la vista previa tonal pueden costar mucho en dibujos complejos. No se ha medido una latencia global máxima.
- Rigging/Puppet crean contornos temporales. La aproximación adaptativa y el skinning pueden plegarse en poses extremas; el presupuesto de importación no garantiza FPS estables. No se ejecutaron pruebas físicas de presión/Wintab.
- Historial: límite de 60 estados, sin presupuesto global de RAM. COW evita copias completas, pero las ramas editadas y los contornos nuevos aún consumen memoria.
- La recuperación y el cargador terminan su operación actual antes de cerrar. Un disco lento o una importación costosa puede retrasar el cierre. La carga diferida de sesiones restauradas sigue usando una lectura síncrona al activar esa pestaña.
- Clippy se ejecutó sobre todos los targets. Permanecen sugerencias de estilo/refactorización (principalmente `collapsible_if`, inicializadores y número de argumentos); no se aplicó una reescritura masiva ajena a los fallos encontrados.
- No se incorporó un fuzzer ni una colección de FLA reales de distintas versiones de Animate. Son los siguientes pasos para ampliar la cobertura del formato, especialmente motion-object XML, máscaras, trazos escalados y texto TLF.

## Verificación reproducible

```text
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked
cargo build --release --locked
cargo test --release --locked export_all_containers_with_ffmpeg -- --ignored --nocapture
cargo test --release --locked benchmark_audit_shared_validation -- --ignored --nocapture
```

El benchmark valida 200 trazos vectoriales compartidos durante 10.000 fotogramas, diez iteraciones. Mide validación en CPU, sin dibujo, GPU, recuperación ni lectura de disco. No mide FPS de la aplicación ni se compara con Animate. Las cifras están en `PERFORMANCE.md`; la validación de Windows se registra en la PR.
