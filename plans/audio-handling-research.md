# Cómo manejan el audio los emuladores de SNES de referencia

Investigación verificada contra el código fuente real (master, julio 2026) de
snes9x, bsnes, ares, Mesen2 y RetroArch-web, comparada con el pipeline actual
de OxideSFC. Objetivo: entender qué hace falta para un audio "correcto"
(sin clicks, sin deriva, latencia acotada) en sesiones de juego largas.

## El problema de fondo (común a todos)

Hay dos relojes físicos distintos e independientes:

- **Productor**: la emulación, paseada por reloj de pared o vsync
  (~60.0988 fps NTSC), genera muestras a la tasa nativa del DSP.
- **Consumidor**: el DAC de la tarjeta de sonido, con su propio cristal.

Aunque ambos digan "32 kHz", difieren en decenas/cientos de ppm. Sin
corrección, el buffer intermedio se drena (underruns periódicos = clicks) o
se llena (latencia creciente y luego descartes). Todos los emuladores maduros
resuelven esto con **control dinámico de tasa (DRC)**: micro-ajustar el ratio
de resampleo (≤ ±0.5%, inaudible) según el nivel de llenado del buffer.

## 1. Reloj del DSP: 32.040 Hz derivados del reloj maestro emulado

| Emulador | Tasa DSP | Cómo la deriva |
|---|---|---|
| snes9x | **32.040 Hz** | SPC700 esclavizado a ciclos del CPU con ratio entero exacto `15664/328125` (NTSC) → SMP a 1.025.280 Hz; DSP tick 1:1 con el SMP; 1 muestra cada 32 ciclos DSP. `apu/apu.cpp` |
| bsnes / ares | **32.040 Hz** | `apuFrequency = 32040.0 * 768.0` (= 24.606.720 Hz, el cristal real mide ~24,607 MHz, no los 24,576 nominales). Cothreads (libco) con reloj virtual normalizado de 64 bits; CPU y APU en dominios de reloj separados. `sfc/system/system.hpp` |
| Mesen2 | **32.000 Hz** (por diseño) | `SpcSampleRate = 32000`; `_clockRatio = (rate*64)/masterClockRate` y el SPC corre "catch-up" hasta el objetivo derivado del master clock. Elige 32000 para que audio y video no deriven *por construcción*. `Core/SNES/Spc.cpp` |

Puntos comunes:
- **Nunca free-running**: el APU siempre se esclaviza al reloj maestro emulado
  con un ratio exacto (entero + resto acarreado), así el tiempo de audio y el
  de video no pueden derivar *dentro* de la emulación.
- **Sincronización perezosa**: el SMP/DSP corre en catch-up en cada acceso a
  los puertos $2140-$2143, al final de cada scanline/frame, y antes de
  serializar — no por ciclo.
- OxideSFC genera 32.000 Hz exactos (1 muestra / 32 pasos SPC700): misma
  decisión que Mesen2, autoconsistente. snes9x/bsnes usan 32.040 por fidelidad
  al hardware (la música suena un 0,125% más aguda = como la consola real).

## 2. Resampleo: interpolador con estado persistente

- **snes9x**: Hermite cúbico de 4 puntos; el `Resampler` ES el ring buffer
  (`apu/resampler.h`). Ratio = 32040 / tasa_host, cursor fraccional.
- **bsnes/ares**: cúbico Catmull-Rom por canal (`nall/dsp/resampler/cubic.hpp`),
  cola de salida de ~20 ms. Filtro anti-alias (3 biquads Butterworth) **solo**
  al submuestrear ≥2×; para 32040→44,1k/48k (sobremuestreo) no aplica ninguno.
  Sin filtro DC en la ruta SNES.
- **Mesen2**: Hermite (`Utilities/Audio/HermiteResampler.cpp`) con historia de
  4 muestras, fase fraccional persistente entre buffers y cola `_pendingSamples`
  para no descartar nada. 1:1 exacto = passthrough memcpy.

Claves: la fase y la historia del interpolador **sobreviven entre buffers y
entre cambios de ratio** (si se resetean, hay click); el cambio de ratio está
acotado (≤±0,5%) así que nunca es una discontinuidad.

## 3. Control dinámico de tasa (DRC) — el mecanismo central

Origen: paper de Hans-Kristian Arntzen (libretro,
docs.libretro.com/guides/ratecontrol.pdf) y artículo de Near/byuu
(bsnes.org/articles/dynamic-rate-control).

- **bsnes** (`ruby/audio/audio.cpp`), fórmula exacta con `maxDelta = 0.005`:
  ```cpp
  dynamicFrequency = ((1.0 - maxDelta) + 2.0 * fillLevel * maxDelta) * frequency;
  ```
  i.e. `ratio = 1 + 0.005·(2·fill − 1)`: buffer al 50% → ratio 1.0 exacto;
  vacío → −0,5%; lleno → +0,5%. Requiere: vsync ON, audio-blocking OFF.
- **snes9x** (`S9xUpdateDynamicRate`, `apu/apu.cpp`): proporcional puro sobre el
  espacio libre del buffer del driver, equilibrio en 50%, límite por defecto
  `DynamicRateLimit = 5` → ±0,5%:
  ```c
  multiplier = 1.0 + (limit * (buffer_size - 2*avail)) / (1000.0 * buffer_size);
  ```
- **RetroArch** (`audio_driver_compute_rate_adjust`): idéntico en espíritu,
  `DEFAULT_RATE_CONTROL_DELTA 0.005`, más `max_timing_skew 0.05` que dobla la
  suposición de refresco de video hasta ±5% para que el DRC solo absorba el
  residuo.
- **Mesen2** (`SoundResampler::GetTargetRateAdjustment`): el más sofisticado —
  controlador **proporcional + integral** sobre la **latencia medida** (media
  móvil de 60 frames del gap read↔write del dispositivo):
  - objetivo = latencia configurada (60 ms por defecto), banda muerta ±3 ms;
  - término P: 0,003125% por ms de error fuera de la banda, tope ±0,25%;
  - término I (`_underTarget`): acumula lentamente, tope ±0,0625% — "aprende"
    el reloj real de la tarjeta para que el término P descanse en cero;
  - histéresis: el ajuste P se libera solo al volver a ±1 ms;
  - deshabilitado al grabar WAV/AVI o con velocidad ≠ 100%.
  El ajuste se aplica a la tasa de SALIDA del resampler; el dispositivo sigue
  creyendo que reproduce a 48 kHz.

## 4. Estrategia de sincronización A/V: elegir UN maestro

Tres regímenes (snes9x implementa los tres; bsnes ofrece dos presets):

1. **Video-maestro + DRC en audio** (el moderno por defecto): la emulación se
   pasea por timer de frame o vsync; el audio corre libre hacia el buffer del
   driver y el DRC dobla el ratio ±0,5%. Es el esquema de Mesen2 (FrameLimiter
   por reloj de pared con tiempo objetivo absoluto acumulado, nunca bloquea en
   audio) y el preset "Dynamic Rate Control" de bsnes.
2. **Audio-maestro** ("sync to sound"): la emulación BLOQUEA en el write del
   dispositivo (WASAPI `WaitForSingleObject(INFINITE)` en bsnes; ALSA en modo
   blocking en snes9x). El video corre a lo que dicte el cristal del DAC.
   Recomiendan desactivar vsync en este modo.
3. **Vsync-locked con tasa de entrada medida** (GTK snes9x "auto input rate"):
   si se lockea al refresco del monitor, se reescala la tasa nominal:
   `input = refresh * 32040 / 60.09881…` (clamp ±5%), y el DRC absorbe solo el
   residuo.

Nunca ambos maestros a la vez (audio-blocking + vsync = tirones).

## 5. Buffers, underrun y arranque

- Tamaños: snes9x 32–64 ms típico; bsnes WASAPI default 40 ms (opciones
  0–100); ares SDL3 default 40 ms (opciones 10–80, chunk de dispositivo 10 ms);
  Mesen2 60 ms objetivo, ring del dispositivo = 2× latencia pedida;
  RetroArch-web con AudioWorklet: 64 ms.
- **Arranque diferido** (Mesen2): no empezar a reproducir hasta tener ≥ media
  latencia objetivo encolada — evita el click de underrun inicial.
- **Underrun**: reproducir lo que hay y luego **silencio** (snes9x re-arma con
  media buffer de silencio; nunca repetir muestras viejas). Mesen2 salta el
  cursor de escritura hacia delante y cuenta el evento en estadísticas.
- **Deriva gruesa** (Mesen2): si |latencia − objetivo| > 50 ms → stop limpio,
  vaciar, re-bufferizar; el régimen fino queda para el DRC.
- **Pausa / load-state**: parar o pausar el dispositivo explícitamente y dejar
  de alimentar; limpiar colas para no reproducir audio viejo ("audio blip al
  cargar un save state" — comentario literal en Mesen2). Ducking de volumen =
  multiplicación, nunca parar el stream.

## 6. El caso web/webview (relevante para Tauri)

- **ScriptProcessorNode está deprecado** porque su callback corre en el hilo
  principal: cualquier render de React, GC o resolución de `invoke` larga
  retrasa el callback más allá del deadline del hardware → dropout.
  **AudioWorklet** corre en el hilo de audio real, en quanta fijos de 128
  frames (~3 ms de presupuesto por quantum a 44,1 kHz).
- Patrón canónico: **ring buffer dentro del AudioWorkletProcessor**, alimentado
  con `port.postMessage(chunk, [chunk.buffer])` (transferible, 1 mensaje por
  frame de video), o con `SharedArrayBuffer` + Atomics (ringbuf.js de padenot;
  requiere COOP/COEP — Tauri ≥2.1 lo permite vía `app.security.headers`).
- RetroArch migró en 2025 de `rwebaudio.c` (cola de `AudioBufferSourceNode`
  desechables de 10 ms) a un driver AudioWorklet real (PR #17750) y con eso
  bajó la latencia por defecto a 64 ms.
- Ningún emulador JS popular implementa DRC (binjgb usa colchón de 100 ms;
  wasmboy estira `playbackRate` grueso; jsnes-web descarta/rellena) — la
  oportunidad es implementarlo en el consumidor, dentro del worklet.
- IPC Tauri: los `Vec<i16>` serializados a JSON son lentos para PCM; usar
  `tauri::ipc::Response::new(bytes)` (ArrayBuffer binario) o
  `tauri::ipc::Channel<&[u8]>` para streaming push. (~2 KB binarios vs ~12 KB
  de JSON por frame a 60 fps.)

## 7. OxideSFC hoy vs. las referencias

Lo que ya está bien:
- DSP a 32.000 Hz exactos derivado del clocking del SPC700 (misma decisión que
  Mesen2, autoconsistente).
- `AudioContext` a 32 kHz → reproducción 1:1 sin resampleo propio.
- Ring buffer O(1) con interpolación lineal y cursor fraccional
  (`AudioService.processAudio`) — es exactamente la maquinaria que un DRC
  necesita; hoy solo la usa el multiplicador de velocidad.
- Underrun con decaimiento ×0,95 (mejor que silencio brusco), overflow
  descartando lo más viejo.

Lo que falta (por prioridad):
1. **DRC**: no hay corrección productor(reloj de pared)↔consumidor(DAC). Con
   ring de ~1 s la deriva se convierte en latencia creciente hasta descartes,
   o en underruns periódicos. Arreglo mínimo: en el bucle de salida,
   `step = playbackRate * (1 + 0.005·(2·fill − 1))` con setpoint ~50 ms.
2. **AudioWorklet** en vez de `ScriptProcessorNode` (mover ring + interpolador
   al processor casi tal cual; alimentar con postMessage transferible).
3. **IPC binario** (`tauri::ipc::Response`) en vez de `Vec<i16>`→JSON.
4. **Objetivos de buffer**: capacidad ~100 ms, setpoint 50 ms, contadores de
   underrun/overrun visibles en un overlay de dev.
5. **Limpiar colas en pausa/load-state** (hoy `stop()` resetea el ring, pero
   conviene auditar el flujo de pausa y load_state).
6. Largo plazo: hilo de emulación en Rust auto-paseado (la lógica de
   `frame_debt` ya existe en `controller.rs`) que empuje audio por
   `tauri::ipc::Channel`, para que rAF solo lea video (hoy un tab/ventana
   ocluida estrangula rAF y mata el audio).

## Estado de implementación (2026-07-19)

Todo lo accionable de la sección 7 quedó implementado y verificado en vivo
(SMW corriendo, midiendo con el hook dev `window.__oxidesfcAudioStats`):

1. ✅ DRC en el consumidor: fórmula de bsnes (±0,5%, setpoint = latencia
   configurada, 60 ms por defecto) dentro del AudioWorkletProcessor,
   multiplicada por el `playbackRate` del control de velocidad.
2. ✅ AudioWorklet reemplaza a ScriptProcessorNode (processor inlineado vía
   Blob URL; ring + interpolador en el hilo de audio; stats de
   underrun/drop/fill/drc ~1×/s). `initialize()` es single-flight (una
   carrera con React StrictMode construía el nodo sobre un contexto sin el
   módulo registrado).
3. ✅ IPC binario: `get_audio_samples` devuelve `tauri::ipc::Response`
   (bytes i16 LE → `ArrayBuffer` → `Int16Array`), sin JSON por muestra.
4. ✅ Limpieza de colas en start/stop/load-state (Rust) y clear del ring en
   load-state (TS).
5. ✅ `get_video_frame` devuelve `null` si no hay frame nuevo (serial de
   frame en el controller): a 250 Hz de rAF se ahorran ~190 encodes base64
   de ~230 KB por segundo que competían con el stepping y estrangulaban el
   audio.
6. ✅ `[profile.dev.package.oxidesfc-core] opt-level = 3`: el core debug
   tardaba ~20 ms por frame emulado (SMW) — imposible sostener 60 fps; con
   el core optimizado los underruns pasaron de ~7/s a ~4/min en el entorno
   dev instrumentado.

### Segunda ronda (fidelidad del DSP y estabilidad del buffer)

La primera ronda arregló el *transporte* del audio; esta arregla la
*síntesis* y la recuperación del buffer. Auditando el DSP contra
`SPC_DSP.cpp` de bsnes aparecieron cuatro defectos de fidelidad que
explicaban por qué el audio "suena pero no como bsnes":

1. ✅ **Orden de nibbles BRR invertido** (`apu.rs`, `BrrDecoder::decode`). El
   hardware reproduce nibble alto de cada byte y luego el bajo
   (H0,L0,H1,L1,…); el código emitía los ocho bajos y después los ocho
   altos. Eso desordenaba temporalmente cada bloque de 16 muestras de
   *cada* sample de *cada* juego y además hacía correr el historial de los
   filtros de predicción en el orden equivocado, así que las amplitudes
   decodificadas también salían mal. Se oía como aspereza/granulado
   constante en todo instrumento muestreado. Era el defecto de calidad
   dominante.
2. ✅ **Interpolación gaussiana real** en vez de Catmull-Rom (tabla `GAUSS`
   de 512 entradas transcrita de bsnes, con el truncado a 16 bits a mitad
   de la suma y el borrado del bit bajo que hace el hardware). La gaussiana
   es un filtro paso-bajo que el contenido BRR asume; Catmull-Rom es más
   brillante, deja pasar el ruido de cuantización del BRR y sobreoscila.
3. ✅ **Generador de ruido (LFSR de 15 bits) y NON ($3D)**: no existían. Las
   voces marcadas para ruido reproducían su sample BRR, así que percusión
   (charles, cajas, platillos) y efectos basados en ruido (viento, lluvia,
   explosiones) estaban mal o ausentes en buena parte de la biblioteca.
4. ✅ **Modulación de pitch (PMON $2D)**: tampoco existía; los efectos de
   vibrato/growl sonaban a pitch fijo.

Y de precisión/robustez: saturación a 16 bits **por voz** (el hardware
satura tras cada suma, no al final de la mezcla), FLG bit 6 (mute) y bit 7
(soft reset), escritura de ENVX/OUTX ($x8/$x9) que los drivers consultan
para fades y robo de voces, y el latch de EDL ($7D) al dar la vuelta el
buffer de eco en vez de reasignarlo a ceros (que producía un corte de eco
con clic al cambiar de canción).

Además, el SPC700 ejecutaba **una instrucción completa por ciclo** y
descartaba el coste devuelto por `step()`, es decir ~3.5× su rendimiento
real. El tempo salía bien porque timers y divisor del DSP estaban
calibrados en esas unidades, pero cualquier driver cuyo timing dependa de
cuánto trabajo cabe entre dos ticks de timer veía una máquina 3.5× más
rápida. Ahora cada instrucción cobra sus ciclos reales y los timers se
mueven por ciclo (con el sobrante acarreado entre llamadas).

Del lado del reproductor, la causa de los "fallos ocasionales" era la
**ausencia de prellenado**: el ring empezaba —y tras cada underrun volvía a
empezar— casi vacío, y lo único que empujaba el nivel hacia el objetivo era
el diferencial ±0.5% del DRC, unos 160 frames/s de margen: subir de una
ráfaga de ~533 frames a 1920 tardaba ~12 segundos. Durante toda esa ventana
cualquier tirón del hilo principal volvía a vaciarlo, así que los glitches
llegaban en racimos. Ahora el worklet hace *priming* (silencio, sin
consumir, hasta llenar el objetivo) al arrancar, al hacer `clear` y tras
cada underrun, de modo que cada incidente es una pausa acotada (~4 frames)
y luego un buffer que sí absorbe jitter. También se corrigió que el default
de `settingsStore` (50 ms, el valor que medimos como insuficiente) pisara
el 60 ms calibrado —al ser `50` truthy, el fallback `|| 60` nunca aplicaba—
y que una respuesta obsoleta de `getFrame` dejara el buffer anterior en el
store, con lo que el bucle de render lo encolaba **una segunda vez** (un
trozo de ~16 ms repetido, además del hueco de las muestras descartadas).

Pendiente (mejora de fondo, no bloqueante): mover el productor a un hilo
Rust auto-paseado que empuje audio por `tauri::ipc::Channel`, para
independizar el bombeo del rAF del webview (ventanas ocluidas/minimizadas).
Y, como paridad fina con bsnes, resampler cúbico en el worklet (hoy lineal,
irrelevante mientras el contexto corra a 32 kHz) y ESA/eco dentro de la RAM
del APU.

## Fuentes principales

- snes9x: `apu/apu.cpp`, `apu/resampler.h`, `apu/bapu/dsp/SPC_DSP.cpp`,
  `gtk/src/gtk_sound.cpp`, `win32/CXAudio2.cpp` — github.com/snes9xgit/snes9x
- bsnes: `sfc/system/system.hpp`, `sfc/dsp/dsp.cpp`, `emulator/audio/*`,
  `nall/dsp/resampler/cubic.hpp`, `ruby/audio/audio.cpp`, `ruby/audio/wasapi.cpp`
  — github.com/bsnes-emu/bsnes
- ares: `ares/sfc/dsp/dsp.cpp`, `ares/scheduler/thread.cpp`,
  `ruby/audio/audio.cpp` (SDL3; DRC vestigial en master, verificado en v138)
  — github.com/ares-emulator/ares
- Mesen2: `Core/SNES/Spc.cpp`, `Core/Shared/Audio/{SoundMixer,SoundResampler,
  BaseSoundManager}.cpp`, `Utilities/Audio/HermiteResampler.cpp`,
  `Windows/SoundManager.cpp`, `Sdl/SdlSoundManager.cpp` — github.com/SourMesen/Mesen2
- RetroArch: `audio/audio_driver.c`, `audio/drivers/audioworklet.c` (PR #17750),
  `audio/drivers/rwebaudio.c`, `config.def.h` — github.com/libretro/RetroArch
- Artículos: bsnes.org/articles/dynamic-rate-control,
  docs.libretro.com/guides/ratecontrol.pdf,
  developer.chrome.com/blog/audio-worklet-design-pattern,
  github.com/padenot/ringbuf.js, v2.tauri.app/develop/calling-rust
  ("Returning Array Buffers")
