# Base de rendimiento

Esta iteración reduce trabajo durante la edición sin cambiar el formato de los proyectos. No constituye una comparación con Adobe Animate.

## Datos e historial

`SharedVec<T>` utiliza `Arc<Vec<T>>` con copia al modificar. El documento comparte capas, las capas comparten la lista de fotogramas, cada fotograma comparte sus trazos y cada trazo comparte sus puntos. Crear un estado del historial es O(1) y no duplica geometría. Al editar se copian los contenedores compartidos de la rama afectada; cambiar los puntos de un trazo copia esos puntos si otra instantánea los conserva. Duplicar un fotograma comparte inicialmente el dibujo y mantiene independientes las ediciones posteriores.

Esto reduce memoria, pero no establece un límite de bytes para el historial: siguen existiendo hasta 60 estados y los trazos realmente modificados necesitan memoria propia. El coste de modificar un contenedor compartido depende del número de elementos de ese contenedor.

## Pincel

`StrokeSmoother` conserva el muestreo por distancia y actualiza solo la cola afectada por el filtro de ventana y la interpolación. Los puntos anteriores ya estables se conservan. Una muestra que salta una distancia grande sigue requiriendo trabajo proporcional a los nuevos puntos generados. El filtro original se conserva únicamente en las pruebas como referencia; se compara el resultado completo de cada prefijo del trazo, incluidos extremos, saltos, muestras repetidas y diferentes intensidades.

El renderizado y la teselación del trazo completo siguen teniendo coste proporcional a su longitud. La mejora del filtro no elimina ese coste.

## Línea de tiempo y reproducción

Solo se generan dibujos, interacciones y textos de ayuda para las filas y columnas visibles. La extensión completa del contenido se conserva para desplazar la vista. La prueba de interfaz compara el número de formas generadas con un documento pequeño y otro de 100 capas × 10.000 fotogramas. La reproducción solicita repintados según el siguiente fotograma y conserva el tiempo sobrante para reducir deriva.

## Recuperación

Un único trabajador serializa y escribe fuera del hilo de interfaz. Mantiene como máximo una instantánea pendiente, reemplazada por la edición más reciente, además de la que esté escribiendo. Agrupa cambios durante 250 ms sin posponerlos indefinidamente. Al cerrar normalmente, escribe la última instantánea y espera al trabajador. Una interrupción abrupta puede perder los cambios aún pendientes.

El JSON se escribe con un búfer de tamaño acotado a un archivo temporal, sin construir otro documento JSON completo en memoria, y después se renombra. Los proyectos JSON anteriores siguen siendo legibles y el formato FLA conserva sus limitaciones existentes. Guardar FLA manualmente todavía se ejecuta en el hilo principal.

## Validación reproducible

Desde una terminal con Rust estable y MSVC/Windows SDK disponibles:

```powershell
cargo fmt --all -- --check
cargo test --locked
cargo test --release --locked benchmark_ -- --ignored --nocapture --test-threads=1
```

Las mediciones manuales no tienen umbrales temporales en las pruebas automáticas: cambian según CPU, carga, antivirus y modo de compilación. El benchmark del pincel compara 5.000 muestras con el filtro anterior en cada prefijo; el del historial informa tiempo por instantánea frente a las copias de geometría del modelo anterior para 2.400.000 puntos lógicos. Son mediciones de componentes, no FPS globales ni resultados de Animate.

Antes de añadir funciones grandes, medir latencia de entrada y tiempo de cuadro (mediana, p95 y p99), memoria residente y picos al guardar con documentos representativos. Quedan pendientes caché de geometría/teselación, descarte espacial de trazos fuera de vista, límite del historial por memoria, formato de recuperación incremental y pruebas comparativas con Animate usando los mismos documentos y equipo.


## Medición local del 30 de octubre de 2026

Windows x64, AMD Ryzen 5 3550H (4 núcleos / 8 hilos), Rust 1.99.1. Perfil `release` predeterminado, sin opciones adicionales de CPU ni LTO. Una ejecución por benchmark, secuencial; el tiempo por instantánea es un promedio de las iteraciones indicadas. Son resultados sintéticos locales, sin comparación con Animate.

| Escenario | Referencia anterior | Implementación actual |
| --- | ---: | ---: |
| Suavizar cada prefijo de un trazo sintético de 5.000 muestras | 698,0637 ms en total | 2,1456 ms en total |
| Crear instantánea con 2.400.000 puntos lógicos | 7,027081 ms por copia de geometría, 100 iteraciones | 12 ns por instantánea compartida, 100.000 iteraciones |
| Formas de la interfaz con 10 capas × 100 fotogramas frente a 100 × 10.000 | — | 846 frente a 846 en la misma ventana, prueba sin optimizar |

El primer benchmark conserva el cálculo original para comparar y verifica igualdad de todos los puntos finales. La reducción de tiempo del filtro es de aproximadamente 325,3 veces en este escenario; no mide el renderizado, la teselación, el guardado ni la latencia global del editor. La referencia del historial reproduce las copias profundas de geometría del modelo anterior, no una ejecución del programa antiguo completo; además, la geometría fuente está compartida en la preparación del benchmark. Los 12 ns no incluyen editar, liberar geometría antigua ni serializar una recuperación.

Validación realizada: 27 regresiones superadas tanto en desarrollo como en `release`, y los 2 benchmarks manuales superados en `release`; comprobación de formato Rust y sintaxis de PowerShell. El ejecutable de desarrollo permaneció abierto ocho segundos sin errores de arranque con una recuperación temporal aislada. Esta comprobación no sustituye una sesión de dibujo manual.

El lanzador normal abre `target/release/libretoon.exe`. [Desarrollar.cmd](scripts/dev/Desarrollar.cmd) conserva el ciclo rápido de compilación de desarrollo. Al guardar código, el reinicio del editor permite el cierre normal para que termine la recuperación pendiente.


## Iteración de dibujo vectorial

El filtro incremental anterior se mantiene durante el dibujo. Ahora se procesan todas las muestras PointerMoved de cada cuadro, no solo la posición final. Al terminar se ejecuta un ajuste de mínimos cuadrados con subdivisión acotada y tangentes continuas. El pincel vectorial conserva esos nodos; el estándar genera el área de tinta una sola vez al finalizar. El trabajo de cierre del estándar depende de la longitud y de las intersecciones del recorrido.

La geometría preparada se conserva en coordenadas del documento. La caché distingue identidad de los datos compartidos, topología y nivel de detalle; cambiar solo el color no vuelve a teselar. Conserva aproximadamente hasta 32 MiB de polilíneas y mallas y descarta geometría que deja de usarse. No es un límite global de RAM. Los borradores evitan la caché para no mantener una referencia que obligue a copiar el trazo completo en cada muestra.

Los recortes de rellenos usan operaciones booleanas de polígonos; las curvas del borde se aproximan con tolerancia de 0,04 unidades. Los contornos se cortan por sus intersecciones con el marco y subdivisión de De Casteljau: mantienen sus tiradores. Estas operaciones se ejecutan al terminar el marco de selección, no en cada movimiento del cursor.

Medición local adicional, mismo Ryzen 5 3550H y Rust 1.99.1, perfil release predeterminado, ejecución secuencial única:

| Componente, trazo sintético de 5.000 muestras | Resultado |
| --- | ---: |
| Ajuste a 24 segmentos Bézier | 2,4306 ms |
| Convertir ese recorrido en tinta de relleno de 8 unidades de grosor | 19,3265 ms |
| Recortar el relleno resultante con un marco | 0,9341 ms |
| Filtro incremental durante todos los prefijos | 2,0469 ms |
| Filtro anterior durante todos los prefijos | 649,1774 ms |

Los tiempos de ajuste y relleno corresponden al cierre del trazo, no a cada muestra. No incluyen renderizado ni latencia de entrada global. Las diferencias respecto a la medición anterior dependen de la carga del equipo y no sustituyen una comparación estadística. La prueba de línea de tiempo sigue mostrando el mismo trabajo con 1.000 y 1.000.000 de celdas: 872 formas de interfaz en ambos casos tras añadir los controles nuevos.

Se añadieron regresiones de dibujo fuera del escenario, conservación de varias muestras en un cuadro, pluma, modos de rectángulo, selección parcial, huecos y conservación de área, subdivisión de contornos cúbicos, continuidad del ajuste, caché, curvas y rellenos en FLA, y rechazo de metadatos nativos desactualizados. La comprobación visual con un perfil aislado confirmó dibujo fuera del blanco, separación y desplazamiento de un fragmento y edición de un tirador Bézier.

Siguen pendientes perfiles globales de escenas representativas, selección espacial para documentos grandes, fusión automática de rellenos superpuestos y comparación equivalente con Animate. La caché no elimina el coste de transformar y enviar los vértices visibles a la interfaz gráfica.


## Iteración de interfaz, relleno y anchura

El modelo conserva las dimensiones del escenario y sus FPS con valores predeterminados compatibles con los proyectos anteriores. Cambiar resolución no escala geometría; el encaje y las conversiones entre pantalla y documento usan el centro y tamaño actuales.

Los perfiles de anchura guardan hasta 128 puntos normalizados por longitud, con multiplicadores de 0 a 4 e interpolación suave. Los puntos intermedios necesarios para representar el perfil se muestrean incluso sobre segmentos rectos. El contorno conserva su eje; su silueta se calcula al invalidarse y se guarda en la caché de geometría. La identidad del perfil y el grosor base forman parte de la clave de la caché. Colores distintos reutilizan la geometría. Los recortes remapean el perfil al subtrazado seleccionado.

El bote trabaja solo sobre el fotograma de la capa activa. Construye las regiones delimitadas por la tinta y selecciona la que contiene el clic. Los puentes para cerrar huecos son temporales. Las caras generadas por el bote no se reutilizan como barreras, evitando que el área se contraiga al volver a colorearla. Esta implementación recorre los contornos activos al hacer clic; no usa un índice espacial y escenas con muchos contornos necesitan perfiles adicionales.

Medición release en el Ryzen 5 3550H y Rust 1.99.1 usados en las pruebas anteriores: promedio de 100 ejecuciones por operación, secuenciales.

| Operación | Tiempo medio |
| --- | ---: |
| Silueta de contorno ondulado procedente de 2.000 muestras, con eje Bézier ajustado | 0,158595 ms |
| Bote dentro de un rectángulo de 400 × 300 unidades | 0,078634 ms |

Son escenarios sintéticos pequeños y no miden latencia global, trabajo gráfico ni un documento de producción. Las regresiones comprueban resolución y FPS en FLA/SVG, perfiles no válidos, conservación de área hueca en contornos cerrados, recortes con perfil, cierre de huecos por nivel, recoloración, duración reversible y ensanchado local con deshacer. Una regresión adicional verifica Mayús+W con pulsación y liberación en el mismo cuadro.

La revisión visual confirmó la limpieza del panel derecho, los controles de FPS y duración en la línea de tiempo, los presets de resolución y el aspecto del trazo de puntas finas. La revisión de ventanas se detuvo cuando se recibió Escape. El zoom de cabecera se corrigió para mostrar la escala real y los ajustes de documento se centraron después de esa revisión.


## Corrección de líneas en puntas de anchura cero

La silueta variable era correcta, pero el suavizado la rodeaba con una línea cerrada de egui. Sus uniones en picos casi colineales generaban vértices muy alejados de la tinta. La regresión de renderizado reprodujo una salida de unos 50 puntos de pantalla más allá de la punta a escala 0,2.

Los rellenos y contornos variables ahora usan una franja de antialiasing explícita, con extrusión limitada a 1,5 píxeles físicos y manejo de huecos y orientación del contorno. No se modifica el eje, el perfil ni la geometría guardada. Las pruebas comprueban la malla final que entrega egui, no solo el polígono del documento: puntas finas, extremos casi nulos, anchura cero interior, curvas, siluetas exportadas, rotaciones, zoom y varias densidades de píxeles.


Medición del borde sobre una silueta Bézier obtenida de 2.000 muestras y perfil de puntas finas, con geometría ya preparada: promedio de 200 cuadros en release. Incluye preparar las formas de borde y su teselación, sin dibujar el cuerpo ni enviar datos a la GPU. El borde anterior tardó 62,613 µs por cuadro y el limitado 77,342 µs: unos 14,729 µs adicionales por contorno en este escenario. La prioridad de esta corrección es eliminar las uniones sin límite; no se afirma una mejora de rendimiento.

Las tres regresiones nuevas pasan junto a la batería anterior. Cubren la malla final, distintas escalas y rotación, puntas casi nulas, ceros interiores, rellenos exportados, huecos y varias densidades de píxeles. Los trazos guardados no se reescriben: la corrección se aplica al volver a renderizarlos.

## Herramientas de estudio y exportación

La vista de solo escenario añade cuatro semiplanos de máscara, incluidos los giros de vista. No recorta los datos del documento. Las reglas generan marcas solo para el intervalo visible, con un límite adicional de 2.001 marcas por regla. Las guías son estado de vista de la sesión.

La simetría duplica únicamente la geometría de la pincelada, forma o trazado afectado. Con ambos ejes activos puede generar cuatro versiones. Los pinceles tonales ejecutan las intersecciones al soltar el gesto, sobre los rellenos del fotograma de la capa activa. Conservan los contornos, pero su coste depende de los polígonos y sus intersecciones; no tienen índice espacial ni un límite de latencia demostrado para dibujos complejos.

La exportación toma una instantánea compartida y renderiza en un trabajador. Usa la teselación del editor y rasterización de triángulos en CPU; mantiene un búfer RGB del fotograma actual y envía fotogramas a FFmpeg por una tubería. No materializa la película completa en memoria. El GIF usa paleta por fotograma. La resolución máxima es 4.096 × 4.096; ese búfer RGB ocupa aproximadamente 48 MiB, además de la geometría, el contexto y la memoria del proceso FFmpeg. Esto no constituye un límite global de RAM. La caché geométrica conserva el límite previo.

Las regresiones verifican máscara con rotación, guías arrastrables, simetría con anclas y perfil, portapapeles/espejo con deshacer, edición tonal acotada al relleno con conservación de área/contornos, atajos con modificadores y renderizado sin capas ocultas. La prueba manual `export_all_containers_with_ffmpeg` codifica MP4/SVG/MKV/AVI/MOV/GIF y decodifica los formatos raster para comprobar el número, orden y color de los fotogramas. No mide rendimiento global ni compara con Animate.

La revisión visual no se completó: el control de aplicaciones se detuvo al recibirse Escape. Las comprobaciones automáticas no sustituyen esa revisión.

### Ajustes posteriores de interfaz

La cabecera unificada envía los comandos nativos de mover, maximizar/restaurar, minimizar y cerrar. Se ha añadido una regresión de los tres botones de ventana. Las reglas ocupan los bordes del área central; su franja se reserva fuera del área de dibujo, con etiquetas enteras cuando procede y texto vertical girado. El trabajo de las marcas sigue acotado al intervalo visible.

Se revisaron renderizados sin ventana de la interfaz principal, las categorías de preferencias, el desplegable de pinceles y la exportación. Estos permiten comprobar colocación, textos y superposiciones, pero no sustituyen una prueba manual de movimiento y redimensionado en Windows. La batería actual supera 83 pruebas automáticas; las seis pruebas manuales permanecen excluidas por defecto.

## Rellenos toon, composición y recursos

Fill Brush recorta la pincelada a los rellenos existentes del fotograma de la capa activa. Los pinceles tonales guardan contadores independientes de sombra y luz en cada fragmento vectorial. La vista previa excluye zonas que ya han alcanzado el nivel permitido. El recorte de la vista previa hace operaciones booleanas durante el gesto: esta ruta necesita perfiles con dibujos complejos y no se afirma una latencia constante. El cierre modifica únicamente los rellenos afectados y mantiene sus contornos.

Las exposiciones conservan los fotogramas físicos compatibles con el formato anterior, con una lista adicional de claves. Al confirmar una edición se comparten las referencias de la exposición afectada; el coste depende de su duración. Los proyectos anteriores siguen cargando como fotogramas independientes cuando no contienen metadatos de claves. Las filas/columnas de la interfaz siguen virtualizadas; la regresión con 100 capas y 10.000 fotogramas mantiene el trabajo gráfico acotado a la ventana.

La composición resuelve únicamente el fotograma solicitado: visibilidad, opacidad, máscaras de silueta, instancias de símbolos y cámara activa. Las máscaras y las instancias transformadas pueden crear geometría temporal y reducir la reutilización de caché. La caché de teselación mantiene su límite previo; no se introduce un límite global de memoria para recursos o historial. Los símbolos y sus capas usan los contenedores compartidos existentes, con ciclos rechazados y profundidad máxima de ocho símbolos. La recuperación durante la edición de símbolos genera una instantánea del proyecto completo con las definiciones actualizadas.

Los degradados lineales subdividen las mallas en los límites de los colores; los radiales usan hasta 32 subdivisiones por lado de cada triángulo. SVG conserva definiciones de degradado. El XFL de interoperabilidad proyecta la escena y aproxima cada relleno degradado con una cuadrícula de 24 por 24 recortes. La información nativa conserva los recursos editables; el límite de 64 MiB del contenedor sigue vigente. El guardado manual FLA continúa en el hilo de interfaz y puede resultar costoso con escenas grandes.

Medición sintética local en perfil release predeterminado, una ejecución secuencial: resolver 200 rectángulos planos tardó de media 95,923 microsegundos por fotograma (200 iteraciones). Aplicar una pincelada toon sobre 200 rellenos rectangulares superpuestos tardó 1,30868 ms por operación (20 iteraciones, restaurando la instantánea entre ellas). No incluye la previsualización, teselación, envío a GPU, latencia global ni una comparación con Animate.

La validación añade regresiones del gesto real de Fill Brush y del bloqueo de capa, límites toon y recortes posteriores, degradados radiales rasterizados, claves/exposiciones, máscaras/opacidad, persistencia de recursos, cortes de cámara contiguos/reordenables y modos de reproducción de símbolos. Se revisaron renderizados sin ventana del timeline, las propiedades, la biblioteca, perspectiva y cámaras; no sustituyen una sesión manual de edición completa en Windows.


## Escenas, interpolación y miniaturas (30/10/2026)

Los contenidos de las escenas y sus capas siguen usando SharedVec. Cambiar de escena intercambia referencias; la recuperación integra la escena activa en la instantánea COW y serializa fuera del hilo de interfaz. Los símbolos son recursos compartidos del proyecto. Reproducción, SVG y raster de vídeo/GIF usan el mismo resolvedor de escenas ordenadas, interpolaciones y cámaras.

Solo se interpolan las claves vecinas del fotograma actual. Los identificadores de objeto se indexan una vez por capa/fotograma para evitar búsquedas cuadráticas. Se conservan anclas, tiradores, huecos, degradados y matrices de símbolos; la interpolación genera geometría temporal, no reescribe todos los fotogramas del documento. Las transformaciones directas materializan una clave al iniciar la modificación. Las miniaturas solo resuelven los elementos visibles y el selector de fotogramas virtualiza sus filas; la caché de teselación tiene el mismo límite de 32 MiB y caducidad por fotograma que el lienzo.

Las pruebas cubren posición/giro/escala, fotograma de símbolo, aceleración, trayectoria, claves de cámara, arrastre real, selección y cuentagotas de degradados, persistencia FLA, orden de escenas y píxeles del raster exportado. La revisión visual usa renderizados sin ventana de ocho estados de interfaz; no equivale a una sesión de edición completa en Windows.

Benchmark reproducible: `cargo test --release --locked benchmark_motion_resolution -- --ignored --nocapture`. Resuelve 200 rectángulos animados durante 200 iteraciones, sin teselación, GPU ni interfaz. El guardado FLA conserva las escenas editables en metadatos y concatena una proyección XFL de sus fotogramas; continúa siendo síncrono y puede ser costoso con películas grandes.

Resultado local de una ejecución secuencial en perfil release predeterminado: **281,083 microsegundos por fotograma resuelto**. Este dato no mide FPS de la aplicación ni es comparable por sí solo con otros editores.


## Inicio, documentos y referencias (noviembre de 2026)

Cambiar de pestaña mueve el contexto completo de edición entre sesiones; el documento y el historial mantienen los contenedores SharedVec. Cada documento tiene recuperación independiente. El listado de proyectos contiene rutas explícitas y carpetas elegidas, sin recorrer el disco completo. Los fotogramas de las tarjetas guardadas se rasterizan fuera del hilo de interfaz con una petición pendiente como máximo. La caché conserva hasta 96 tarjetas de 8 imágenes de 192 × 108: aproximadamente 61 MiB de texeles como máximo, más la sobrecarga de texturas y la memoria temporal del proyecto que procesa el trabajador. Las tarjetas idénticas se reducen a un fotograma. No es un límite global de memoria de la aplicación.

El maniquí tiene 16 articulaciones y una jerarquía de tamaño fijo. No recorre los fotogramas del proyecto para calcular la pose. Sus formas se generan solo en el lienzo y no forman parte del resolvedor de exportación. Las proporciones cambian longitudes, y las rotaciones conservan las longitudes de los huesos.

Los glifos del texto se convierten en contornos al confirmar la edición. El renderizado posterior utiliza la caché geométrica existente, sin volver a cargar la fuente. Las operaciones booleanas sobre texto son atómicas o generan una proyección geométrica al aplicar máscaras. La composición y selección no destruyen el texto editable por fragmentos.

El perfil de presión se limita a 128 puntos y el muestreo de la entrada a 2.048 muestras mediante reducción progresiva. El aerosol genera un máximo de 2.049 sellos por gesto; el grafito hasta cinco fragmentos por sello. La vista previa del aerosol depende de la longitud del gesto, y los documentos con muchas pinceladas translúcidas necesitan perfiles adicionales. No se afirma latencia constante ni una mejora global de FPS. La presión necesita un evento real del dispositivo o un controlador Wintab; no puede validarse físicamente sin tableta.

Validación automatizada añadida: aislamiento de documentos/historial/vista/símbolos, pestañas, curvas redondeadas, persistencia de texto y referencias, exclusión de exportación de bocetos anidados, conservación de longitudes del maniquí, presión y renderizados de propiedades. `cargo test --locked render_studio_review_images -- --ignored --nocapture` genera imágenes PPM para revisión visual; `LIBRETOON_PREVIEWS` elige la carpeta de salida. Estos renderizados no sustituyen una sesión manual con Windows y tableta.

## Rigging experimental

Los esqueletos y las retículas de pesos usan los mismos contenedores COW que el documento. Pintar copia solo las retículas afectadas; no cambia ni duplica las curvas fuente del símbolo. La evaluación y el exportador comparten la misma deformación vectorial. Solo se evalúa el fotograma solicitado y se reutilizan poses de huesos dentro del contexto; no se precalcula toda la animación al arrastrar. Los símbolos anidados tienen su propio contexto de rigging.

La deformación activa genera contornos temporales y su teselación sigue teniendo coste proporcional a la geometría evaluada; no se promete coste cero ni una tasa de FPS. Se limita a 128 huesos, 512 instancias vinculadas y 32 pines por instancia. Cada retícula tiene 289 muestras, hasta cuatro influencias por muestra. IK usa como máximo 32 articulaciones y 32 iteraciones. Las curvas se subdividen adaptativamente con objetivo de error 0,02 unidades del documento y profundidad máxima 12. La visualización de pesos subdivide triángulos con un máximo de 32 pasos por arista. Las pruebas verifican aislamiento COW, mezcla continua, interpolación, IK, conservación de fuente y proyección vectorial exportada; no comparan rendimiento con otras aplicaciones.

## Auditoría e importación FLA (noviembre de 2026)

La apertura de proyectos solicitada desde Archivo/galería usa un único lector en segundo plano. Mantiene una petición pendiente reemplazable y un resultado; las solicitudes repetidas no crean trabajadores ilimitados. La restauración diferida de pestañas anteriores aún realiza su lectura al activarlas. El ZIP se indexa una vez por biblioteca y los fotogramas compartidos se deduplican al validar geometría y construir el grafo de símbolos. La profundidad se valida mediante el máximo de sus dependencias, sin depender del orden de los recursos.

Se impone un presupuesto conservador de dos millones de puntos de control expandidos para evitar DAG de símbolos que multiplican exponencialmente el trabajo. Se calcula con máximos por capa/símbolo y puede rechazar proyectos patológicos antes de renderizar; no es una garantía de FPS ni un presupuesto global de RAM. Importación: hasta 256 MiB de archivo, 64 MiB por entrada ZIP y 128 MiB de XML de biblioteca. La geometría importada tiene un presupuesto adicional de 500.000 elementos/puntos contabilizados. El árbol XML y los diagnósticos también tienen límites. FFmpeg se drena continuamente pero solo conserva 64 KiB de mensajes.

Medición sintética en este entorno Linux x86_64 (AMD EPYC 9V74, tres CPU visibles), Rust estable 1.99.1, perfil release predeterminado. Misma función de prueba en un worktree del commit base `fdc32ee` y en esta revisión: 200 trazos de cuatro puntos, compartidos durante 10.000 fotogramas, diez validaciones secuenciales. Una ejecución por versión, sin control de carga ni comparación estadística:

| Validación del documento | Tiempo medio |
| --- | --- |
| Base `fdc32ee` | 59,79551 ms |
| Validación con deduplicación y controles nuevos | 0,60235 ms |

Comando: `cargo test --release --locked benchmark_audit_shared_validation -- --ignored --nocapture`. Mide CPU de validación, no lectura, renderizado, latencia de entrada ni FPS. No constituye una comparación con Animate. Los costes pendientes y la matriz de compatibilidad están en [la auditoría de importación](docs/AUDIT.md).

## Temas e idiomas (noviembre de 2026)

Las paletas solo afectan a decoraciones de la interfaz. Los datos del dibujo, el fondo blanco del escenario y la modulación de las texturas de previsualización conservan sus colores. La paleta actual y el idioma son estado del hilo de UI; las pruebas de renderizado paralelas no se pisan. El estilo se reconstruye solo cuando cambia el tema, y la cadena de fuentes solo cuando cambia la variante CJK. Los diccionarios se cargan una vez, desde JSON integrado en el ejecutable; no hay peticiones de traducción durante el uso de la aplicación.

La colección Noto Sans CJK integrada ocupa aproximadamente 18,5 MiB de datos de fuente, compartidos como bytes estáticos. La memoria de fuentes/atlas depende de los glifos utilizados; no se afirma un límite global de RAM. El paquete Windows omite una segunda copia del TTC, conservando su licencia. Las regresiones comprueban los seis temas, los once idiomas, contraste de texto y texto secundario, cobertura de glifos, persistencia/migración y que recorrer los temas e idiomas no modifica el documento.

`cargo test --locked render_studio_review_images -- --ignored --nocapture` añade vistas de los temas, preferencias y los tres idiomas asiáticos. Son capturas sin ventana para comprobar fuentes, contraste y colocación; no sustituyen una sesión manual en Windows.


## Ajustes de interacción (2 de noviembre de 2026)

Los destinos de arrastre de biblioteca se dibujan sobre las filas visibles; el indicador flotante solo muestra el recurso y el destino actuales. El menú contextual de fotogramas usa las acciones existentes y conserva la virtualización de filas y columnas. Los paneles usan un tamaño inicial con límites, en lugar de bloquear el tamaño cada cuadro.

Las poses de huesos comparten su lista de símbolos afectados mediante SharedVec. La deformación comprueba esa lista antes de aplicar cada influencia y descarta los pesos desvinculados; no duplica geometría fuente. Desvincular limpia únicamente las celdas de la instancia afectada sin redistribuir los pesos de otros huesos. El pintado modifica solo el hueso activo, con hasta cuatro influencias por celda. La comprobación de pertenencia depende del número de símbolos vinculados; no se afirma una mejora temporal ni FPS constantes.

Las regresiones cubren los arrastres reales de bordes de panel, el orden y visibilidad de herramientas a 1366 × 720 y 1366 × 768, la salida de carpetas con indicador de destino, creación de hijos desde el botón +, desvinculación con pesos obsoletos, aislamiento COW y acciones de timeline con deshacer y capas bloqueadas. Los renderizados de revisión se generan a 1366 × 768.


## Texto, referencias 3D y cámaras (2 de noviembre de 2026)

El editor de texto conserva contornos vectoriales portables y metadatos del cuadro. Las fuentes y contornos se resuelven al confirmar cambios; la edición agrupa el historial de una sesión en el cuadro. La fuente de la interfaz de edición se instala al cambiar de fuente, con los datos compartidos de egui, retirando la anterior. La cantidad de trabajo al escribir depende del contenido del cuadro; no se promete latencia constante para 16.384 caracteres.

El maniquí evalúa una jerarquía fija de 16 articulaciones en tres dimensiones. Sus volúmenes son elipsoides de 10 × 16 celdas, con ordenación por profundidad y sombreado de vértices; no carga mallas externas ni añade datos a la exportación. Posar con ratón usa diez iteraciones sobre dos ángulos, sin cambiar longitudes. El coste es constante respecto al tamaño de la película, pero la malla se prepara en cada cuadro y aún necesita envío a GPU.

El recorte entre cámaras guarda únicamente las dos cámaras del gesto y una instantánea COW del historial. Durante el arrastre restaura ese par antes de evaluar el desplazamiento, evitando perder claves al encoger y volver a extender el corte. Solo recorre las claves de esas cámaras; no duplica el proyecto ni desplaza geometría del dibujo.

El bote aplica cierre morfológico solo al pulsar y únicamente a las barreras temporales. Su coste depende de la complejidad de los polígonos. Las pruebas cubren siluetas de pincel rellenas con huecos, conservan los contornos de entrada y comprueban el rechazo de regiones exteriores.

Medición reproducible: cargo test --release --locked benchmark_3d_mannequin_projection -- --ignored --nocapture. Genera cien cuadros de un maniquí girando sobre un lienzo Full HD en Windows, sin GPU. Incluye evaluación, formas de lienzo y trabajo de egui en CPU; no representa FPS globales ni latencia física.

Resultado local de una ejecución en perfil release predeterminado, Windows x64: 1,607568 ms de media por cuadro para los cien cuadros del escenario anterior. Incluye preparar las formas del lienzo y el maniquí, sin GPU ni una escena de producción. No es una comparación con otras aplicaciones.


## Gizmo de articulaciones (2 de noviembre de 2026)

El gizmo añade tres aros de 64 segmentos a la articulación seleccionada, con tamaño constante en pantalla y geometría local proyectada en 3D. Solo busca impactos en esos segmentos y evalúa las 16 articulaciones; no recorre fotogramas ni geometría del dibujo. Durante el gesto conserva la proyección de los aros y una instantánea COW para deshacer. Los aros casi de canto usan un arrastre tangencial para evitar divisiones inestables.

La rotación local adicional se aplica después de situar la articulación, conservando su pivote y las longitudes de los segmentos. Sus valores se guardan en metadatos nativos, con cero por defecto para documentos antiguos. Las pruebas simulan arrastres de los tres ejes, cancelación, capas bloqueadas y deshacer. La medición de CPU de 1,607568 ms de la sección anterior corresponde al maniquí antes de añadir estos aros, no mide esta revisión.

## Profundidad de capas y desenfoque de cámara

La previsualización usa una única tarea de composición en segundo plano, sin cola de fotogramas acumulados. Reutiliza el resultado mientras no cambian la escena, el fotograma, la cámara o los medios; retiene una instantánea compartida para invalidar la caché con seguridad. El botón Show / Hide Camera Blur evita programar nuevas tareas al ocultarlo y no modifica el documento ni la exportación. La imagen de previsualización limita su lado mayor a 480/800/1200 píxeles según calidad; la exportación conserva la resolución solicitada. Las capas dentro del intervalo enfocado conservan su resolución completa.

Gaussian utiliza filtrado separable; Lens utiliza un núcleo circular muestreado con 8/20/48 muestras según calidad. La composición trabaja con alfa premultiplicado y un margen fuera del encuadre antes de recortar el resultado, para evitar halos negros y permitir que el desenfoque de dibujos exteriores entre en cámara. La geometría del fotograma se resuelve una vez por composición.

Medición local Windows x64, perfil release optimizado predeterminado, escenario de 640 × 360, dos capas rectangulares (una enfocada), fuerza 60, tres composiciones por configuración: Gaussian baja/media/alta: 15,72 / 20,26 / 31,97 ms por fotograma; Lens baja/media/alta: 15,13 / 19,95 / 34,42 ms. Mide composición de software del escenario sintético, no FPS globales ni latencia de entrada. Reproducible con `cargo test --release --locked benchmark_camera_depth_and_blur -- --ignored --nocapture --test-threads=1`.

## Clips de animación
Los recursos reutilizan capas, geometría y rigs mediante los contenedores compartidos existentes. Los bloques son exposiciones de instancias: no se hornea una copia del dibujo por fotograma. La vista previa de arrastre conserva solo el bloque afectado y confirma un único cambio de historial al soltar. Las exposiciones se reconstruyen en esa confirmación, con el límite habitual de 10.000 fotogramas.

La pista conserva una caché por identidad de fotogramas y claves, reteniendo sus propietarios para impedir reutilización accidental de direcciones. Solo se dibujan los bloques que intersectan la vista. La comprobación de colisiones durante el arrastre usa los intervalos ordenados. La edición interna conserva el aislamiento entre el recurso y las capas originales; las variaciones independientes comienzan compartiendo datos y se separan al editar.

## Portapapeles de fotogramas
La copia conserva geometría compartida del intervalo y únicamente sus recursos alcanzables. No retiene los demás proyectos, pistas de medios o capas de la escena. La importación remapea identificadores y reutiliza fotogramas compartidos durante una acción explícita de pegado; primero valida un candidato y después añade una única entrada de historial. Las interacciones de ratón no recorren el paquete del portapapeles.

## Ajustes de vista y duración de exposiciones
Los destinos escalares de ajuste se preparan una vez al iniciar el gesto con la geometría visible del fotograma activo. Las búsquedas durante el movimiento usan listas ordenadas. La cuadrícula limita su dibujo al área visible y adapta el paso al zoom; no altera geometría ni exportaciones. El tirador de duración conserva únicamente índices durante la previsualización y confirma una sola entrada de historial al soltar.

## Temporización de clips
El recorte y el estirado calculan su previsualización únicamente a partir del bloque original del gesto. La fase y la velocidad usan f64; el muestreo de bucle y ping-pong es aritmético y no hornea fotogramas. El borrado confirmado de un recurso es una operación explícita global: conserva una sola entrada de historial, reutiliza las exposiciones filtradas por identidad reteniendo sus propietarios y mantiene los fotogramas de las capas no afectadas.

## Carpetas como grupos
Las transformaciones estáticas de carpeta se componen como matrices de similitud. La previsualización usa la vista por capa y conserva la geometría y la teselación originales; no crea claves ni transforma todos los fotogramas durante un gesto. Los límites temporales se calculan por identidad de capas con propietarios retenidos; cambiar solo el transform de una carpeta no vuelve a recorrer sus fotogramas. El gizmo conserva los límites iniciales durante el arrastre y añade una sola entrada de historial.

Dividir y recolocar son operaciones explícitas limitadas a las capas descendientes. Se comparten las exposiciones originales cuando es posible y se retienen propietarios en las cachés de retiming. Una interpolación que cruza el corte se muestrea únicamente en su intervalo afectado para conservar el aspecto. Los rigs mantienen su reloj de origen por carpeta, sin duplicar esqueletos ni hornear la deformación. Los bloques conservan el límite de 10.000 fotogramas, 100 capas y 200 carpetas; las capas ajenas al grupo no se retiman.


## RefTool y recorte vectorial
Las imágenes de referencia usan bytes compartidos en el documento y una textura/RGBA por ventana activa. Se decodifican en un único trabajador, con presupuestos de bytes y píxeles; al cambiar de documento se descartan resultados obsoletos. Los FLA guardan los bytes fuera del JSON, en entradas binarias. Los movimientos de ventana no duplican imágenes y generan una sola entrada de historial por gesto.
El recorte calcula intersecciones al cambiar de contorno objetivo o invalidarse el documento, conservando propietarios compartidos en la clave. El movimiento sobre ese contorno reutiliza los tramos calculados. Solo la confirmación cambia la geometría, mediante subdivisión Bézier y remapeo del perfil de anchura; no rasteriza ni hornea fotogramas.


## Clave automática al pintar
El conmutador A, desactivado inicialmente y guardado en preferencias, crea una clave en el fotograma actual al confirmar un trazo sobre una exposición. Conserva la pose existente con datos compartidos y usa la misma entrada de historial del trazo. No inserta fotogramas, no desplaza claves y no avanza el cabezal. Si ya existe una clave, la reutiliza. El estado desactivado conserva la edición habitual de la exposición completa.

## Caché de previsualización de composición

La vista conserva hasta 32 texturas de fotogramas, con un presupuesto de 96 MiB para la caché. En pausa, prepara hasta 12 fotogramas siguientes según la memoria disponible, con una sola tarea activa y prioridad para el cabezal. Los proyectos con pistas de vídeo usan la ruta de decodificación bajo demanda. Las claves distinguen el tiempo de los símbolos independientes, la resolución y la generación del documento. Una edición invalida las texturas de la generación anterior.

Medición local en Windows x64, perfil release, con los dos primeros fotogramas de DBGT-Rebuild-v1 a 1920 × 1080 y las luces activas. Preparar ambos llevó 3,07 s. Alternar entre sus texturas 100 veces llevó 0,42 ms de CPU en la prueba de interfaz, sin renderizados ni subidas de textura durante esa fase. No mide FPS en una ventana real, latencia de presentación ni el coste de fotogramas que aún no están en caché.

Para repetirla, establece la variable `LIBRETOON_BENCHMARK_PROJECT` con la ruta del proyecto y ejecuta `cargo test --release --locked benchmark_dbgt_preview_cache -- --ignored --nocapture`.

## Segunda iteración de rendimiento

La preparación manual usa la vista de reproducción y admite la escena o la etiqueta activa. El límite de caché de fotogramas RGB en RAM es configurable entre 128 y 1024 MiB, con 256 MiB por defecto. Las texturas de fotogramas tienen un presupuesto separado de 96 MiB. Preparar una escena con pistas de vídeo queda fuera de esta ruta. El compositor conserva cachés acotadas de imágenes base, normales de relieve dependientes de alfa y prefijos de ajustes. Los relieves con mapa de altura o dependientes del color se recalculan. La selección y el bote descartan candidatos por sus límites antes de comprobar el contorno.

El escritor nativo produce el ZIP directamente sobre el temporal, comprime más los metadatos y las teselas raster, y evita el intento de serialización antigua cuando las exposiciones ya superan el umbral por un límite inferior. La recuperación reutiliza la compresión de recursos idénticos y publica instantáneas completas. El cargador en segundo plano también restaura las pestañas sin sesión en memoria.

Mediciones locales en Windows x64, perfil release, con DBGT-Rebuild-v1. Los primeros dos fotogramas Full HD se prepararon en 2,07 s frente a 3,07 s en la medición anterior. Los últimos cuatro tardaron 9,22 s con reutilización entre fotogramas frente a 11,95 s en la revisión anterior. El primer plano coincidió píxel a píxel con el PNG de referencia y los últimos cuatro coincidieron con renderizados sin caché previa. Los tiempos corresponden a ejecuciones individuales y no incluyen FPS de una ventana real.

El proyecto pasó de 4.849.126 a 4.084.613 bytes y reabrió con los fotogramas y los recursos comprobados. Abrir y validar llevó 83 ms. Serializar y comprimir en memoria llevó 369 ms. Repetir la recuperación con la compresión de recursos ya preparada llevó 178 ms. Esas dos últimas cifras excluyen el tiempo de escritura y sincronización de disco.

El prototipo WGPU de desenfoque de lente procesó una imagen sintética Full HD con 48 muestras. En una RTX 2060, CPU tardó 68,2 ms y GPU 11,4 ms con transferencias. La salida RGBA coincidió byte a byte. La integración interactiva en distintos adaptadores queda pendiente. Un ensayo de decodificación secuencial de vídeo con cadencia variable difirió de la búsqueda por fotograma en ocho de doce muestras con remuestreo fijo y en una de doce al seleccionar por PTS. La vista de vídeo reutiliza fotogramas ya decodificados y conserva la búsqueda original cuando falta una muestra.

Pruebas manuales: `benchmark_dbgt_preview_cache` y `benchmark_dbgt_storage` con `LIBRETOON_BENCHMARK_PROJECT`, `benchmark_lit_camera_cache` con `LIBRETOON_CAMERA_FIXTURE` y una referencia PNG opcional en `LIBRETOON_REFERENCE_FRAME`, y `benchmark_gpu_lens_parity` para el ensayo de GPU. Ejecutar con `cargo test --release --locked <prueba> -- --ignored --nocapture`.
