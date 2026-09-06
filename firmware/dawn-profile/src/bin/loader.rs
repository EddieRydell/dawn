#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;

#[cfg(feature = "i2s-output")]
#[path = "../ws281x_parallel.rs"]
mod ws281x_parallel;

use alloc::{boxed::Box, vec, vec::Vec};
#[cfg(feature = "i2s-output")]
use core::sync::atomic::AtomicBool;
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicU32, Ordering::Relaxed},
};
#[cfg(feature = "i2s-output")]
use dawn_runtime::values::sample_time_from_frame;
use dawn_runtime::{
    sequence::{PreparedSequence, SequenceWorkspace},
    values::SampleTime,
    wire::{HEADER_BYTES, LoadError, LoadLimits, decode_sequence},
};
use embassy_net::StackResources;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read as _, Write as _};
#[cfg(feature = "i2s-output")]
use esp_hal::{
    Async,
    dma::DmaTxBuf,
    gpio::NoPin,
    i2s::parallel::{I2sParallel, TxEightBits},
    system::{Cpu, Stack},
    time::Rate,
};
use esp_hal::{
    clock::CpuClock,
    rng::Rng,
    time::Instant,
    timer::timg::TimerGroup,
    uart::{Config, Uart},
};
use esp_println::println;
use esp_radio::wifi::{self, AuthenticationMethodConfig, PowerSaveMode, sta::StationConfig};
use picoserve::{
    AppBuilder, AppRouter,
    response::{IntoResponse, StatusCode},
    routing::{PathRouter, RequestHandlerService, post_service, put_service},
};
#[cfg(feature = "i2s-output")]
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
static EVALUATION_TASK: AtomicU32 = AtomicU32::new(0);
static EVALUATION_ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "i2s-output")]
static OUTPUT_READY: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "i2s-output")]
static APP_CORE_STACK: StaticCell<Stack<4096>> = StaticCell::new();
#[cfg(feature = "i2s-output")]
static APP_CORE_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

type Playback = (PreparedSequence, SequenceWorkspace, Vec<Vec<u8>>);
type SharedPlayback = Mutex<CriticalSectionRawMutex, Option<Playback>>;
type UploadGate = Mutex<CriticalSectionRawMutex, ()>;

const HTTP_PORT: u16 = 80;
const HTTP_WORKERS: usize = 2;
#[cfg(feature = "i2s-output")]
const OUTPUT_PIXELS: usize = 200;
#[cfg(feature = "i2s-output")]
const OUTPUT_LANES: usize = 4;
#[cfg(feature = "i2s-output")]
const I2S_SAMPLE_RATE: u32 = 2_400_000;
#[cfg(feature = "i2s-output")]
const DATA_SAMPLES: usize = OUTPUT_PIXELS * 3 * 8 * 3;
#[cfg(feature = "i2s-output")]
const RESET_SAMPLES: usize = I2S_SAMPLE_RATE as usize * 300 / 1_000_000;
#[cfg(feature = "i2s-output")]
const DMA_BYTES: usize = DATA_SAMPLES + RESET_SAMPLES;
#[cfg(feature = "i2s-output")]
const OUTPUT_FRAME_RATE: u32 = 120;

#[cfg(feature = "i2s-output")]
type ParallelOutput = I2sParallel<'static, Async>;

const LIMITS: LoadLimits = LoadLimits {
    payload_bytes: 32 * 1024,
    pixels: 1600,
    graph_nodes: 128,
    workspace_bytes: 96 * 1024,
};

#[unsafe(no_mangle)]
fn _esp_alloc_alloc(
    _: &esp_alloc::EspHeap,
    _: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
    pointer: usize,
    _: usize,
) {
    if pointer != 0 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        let task = EVALUATION_TASK.load(Relaxed);
        if task != 0 && esp_radio_rtos_driver::current_task().as_ptr() as u32 == task {
            EVALUATION_ALLOCATIONS.fetch_add(1, Relaxed);
        }
    }
}

#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &esp_alloc::EspHeap, _: usize, _: usize) {}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("LOADER PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

async fn uart_reply(
    uart: &mut esp_hal::uart::Uart<'_, esp_hal::Async>,
    args: core::fmt::Arguments<'_>,
) -> Result<(), ()> {
    let mut line = heapless::String::<192>::new();
    line.write_fmt(args).map_err(|_| ())?;
    line.push('\n').map_err(|_| ())?;
    uart.write_all(line.as_bytes()).await.map_err(|_| ())
}

#[inline(never)]
fn load(bytes: &[u8]) -> Result<Playback, LoadError> {
    let archive_headroom = if cfg!(feature = "i2s-output") {
        Some(0)
    } else {
        bytes.len().checked_mul(8)
    }
    .ok_or(LoadError::Limit)?;
    let workspace_bytes = esp_alloc::HEAP
        .free()
        .saturating_sub(16 * 1024)
        .checked_sub(archive_headroom)
        .ok_or(LoadError::Limit)?;
    let limits = LoadLimits {
        workspace_bytes: LIMITS.workspace_bytes.min(workspace_bytes),
        ..LIMITS
    };
    let sequence = decode_sequence(bytes, limits)?;
    #[cfg(feature = "i2s-output")]
    if sequence.output_widths.is_empty()
        || sequence.output_widths.len() > OUTPUT_LANES
        || sequence
            .output_widths
            .iter()
            .any(|&width| width as usize > OUTPUT_PIXELS * 3 || width % 3 != 0)
    {
        return Err(LoadError::Limit);
    }
    let workspace = sequence.workspace();
    let buffers = sequence
        .output_widths
        .iter()
        .map(|&width| vec![0; width as usize])
        .collect();
    Ok((sequence, workspace, buffers))
}

#[derive(Clone, Copy)]
struct LoaderState {
    playback: &'static SharedPlayback,
    upload: &'static UploadGate,
    token: [u8; 32],
}

fn authorized(state: &LoaderState, request: &picoserve::request::RequestParts<'_>) -> bool {
    let Some(supplied) = request.headers().get("x-dawn-token") else {
        return false;
    };
    let supplied = supplied.as_raw();
    supplied.len() == state.token.len()
        && supplied
            .iter()
            .zip(state.token)
            .fold(0, |difference, (&left, right)| difference | (left ^ right))
            == 0
}

struct UploadSequence;

impl RequestHandlerService<LoaderState> for UploadSequence {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Dawn-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let Ok(_upload) = state.upload.try_lock() else {
            return (StatusCode::CONFLICT, "Another upload is in progress\n")
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        };

        let length = request.body_connection.content_length();
        if !(HEADER_BYTES..=HEADER_BYTES + LIMITS.payload_bytes).contains(&length) {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Sequence exceeds device limits\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length).is_err() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Insufficient upload memory\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        bytes.resize(length, 0);

        let offset = {
            let mut reader = request
                .body_connection
                .body()
                .reader()
                .with_different_timeout(Duration::from_secs(15));
            let mut offset = 0;
            while offset < bytes.len() {
                let read = reader.read(&mut bytes[offset..]).await?;
                if read == 0 {
                    break;
                }
                offset += read;
            }
            offset
        };
        let connection = request.body_connection.finalize().await?;
        if offset != bytes.len() {
            return (StatusCode::BAD_REQUEST, "Incomplete sequence body\n")
                .write_to(connection, response_writer)
                .await;
        }

        let free = esp_alloc::HEAP.free();
        let start = Instant::now();
        match load(&bytes) {
            Ok(playback) => {
                let pixels = playback.0.signals.pixel_count;
                let heap = free.saturating_sub(esp_alloc::HEAP.free());
                let elapsed = start.elapsed().as_micros();
                *state.playback.lock().await = Some(playback);
                (
                    StatusCode::OK,
                    format_args!(
                        "LOADED bytes={} pixels={} heap={} us={}\n",
                        bytes.len(),
                        pixels,
                        heap,
                        elapsed
                    ),
                )
                    .write_to(connection, response_writer)
                    .await
            }
            Err(error) => {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format_args!("REJECT {:?}\n", error),
                )
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}

struct EvaluateFrame;

impl RequestHandlerService<LoaderState> for EvaluateFrame {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Dawn-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        if request.body_connection.content_length() != 4 {
            return (StatusCode::BAD_REQUEST, "Frame body must be one u32 tick\n")
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let mut ticks = [0; 4];
        let offset = {
            let mut reader = request.body_connection.body().reader();
            let mut offset = 0;
            while offset < ticks.len() {
                let read = reader.read(&mut ticks[offset..]).await?;
                if read == 0 {
                    break;
                }
                offset += read;
            }
            offset
        };
        let connection = request.body_connection.finalize().await?;
        if offset != ticks.len() {
            return (StatusCode::BAD_REQUEST, "Incomplete frame body\n")
                .write_to(connection, response_writer)
                .await;
        }

        let ticks = u32::from_le_bytes(ticks);
        let mut active = state.playback.lock().await;
        let Some((sequence, workspace, buffers)) = active.as_mut() else {
            drop(active);
            return (StatusCode::CONFLICT, "REJECT NoSequence\n")
                .write_to(connection, response_writer)
                .await;
        };

        let allocations = ALLOCATIONS.load(Relaxed);
        let evaluation_allocations = EVALUATION_ALLOCATIONS.load(Relaxed);
        EVALUATION_TASK.store(
            esp_radio_rtos_driver::current_task().as_ptr() as u32,
            Relaxed,
        );
        let start = Instant::now();
        let result = sequence.evaluate(SampleTime::from_ticks(ticks), buffers, workspace);
        let elapsed = start.elapsed().as_micros();
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_allocations = EVALUATION_ALLOCATIONS.load(Relaxed) - evaluation_allocations;
        let allocations = ALLOCATIONS.load(Relaxed) - allocations;

        if result.is_err() {
            drop(active);
            return (StatusCode::INTERNAL_SERVER_ERROR, "REJECT Evaluation\n")
                .write_to(connection, response_writer)
                .await;
        }

        let mut crc = crc32fast::Hasher::new();
        for buffer in buffers {
            crc.update(buffer);
        }
        let crc = crc.finalize();
        drop(active);
        (
            StatusCode::OK,
            format_args!(
                "FRAME {} {} {} {} {}\n",
                ticks, crc, elapsed, evaluation_allocations, allocations
            ),
        )
            .write_to(connection, response_writer)
            .await
    }
}

struct WebApp {
    state: LoaderState,
}

impl AppBuilder for WebApp {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        picoserve::Router::new()
            .route("/sequence", put_service(UploadSequence))
            .route("/frame", post_service(EvaluateFrame))
            .with_state(self.state)
    }
}

static SERVER_CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: Duration::from_secs(5),
    persistent_start_read_request: Duration::from_secs(5),
    read_request: Duration::from_secs(3),
    write: Duration::from_secs(5),
})
.keep_connection_alive();

#[embassy_executor::task]
async fn network(mut runner: embassy_net::Runner<'static, wifi::Interface>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn reconnect(mut controller: wifi::WifiController<'static>) {
    loop {
        if controller.is_connected() {
            let _ = controller.wait_for_disconnect_async().await;
            println!("WIFI DISCONNECTED");
        }
        match controller.connect_async().await {
            Ok(_) => println!("WIFI CONNECTED"),
            Err(_) => {
                println!("WIFI RETRY");
                Timer::after_secs(5).await;
            }
        }
    }
}

#[embassy_executor::task(pool_size = HTTP_WORKERS)]
async fn web_server(
    task_id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<WebApp>,
) -> ! {
    let mut tcp_rx = [0; 4096];
    let mut tcp_tx = [0; 1024];
    let mut http = [0; 2048];
    picoserve::Server::new(app, &SERVER_CONFIG, &mut http)
        .listen_and_serve(task_id, stack, HTTP_PORT, &mut tcp_rx, &mut tcp_tx)
        .await
        .into_never()
}

fn token_ascii(token: [u8; 16]) -> [u8; 32] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = [0; 32];
    for (index, byte) in token.into_iter().enumerate() {
        result[index * 2] = HEX[(byte >> 4) as usize];
        result[index * 2 + 1] = HEX[(byte & 0xf) as usize];
    }
    result
}

#[cfg(feature = "i2s-output")]
#[embassy_executor::task]
async fn render_outputs(
    playback: &'static SharedPlayback,
    mut output: ParallelOutput,
    mut ready_buffer: DmaTxBuf,
    mut spare_buffer: DmaTxBuf,
) -> ! {
    let mut frame_index = loop {
        let mut active = playback.lock().await;
        let Some((sequence, workspace, buffers)) = active.as_mut() else {
            drop(active);
            Timer::after_millis(10).await;
            continue;
        };
        sequence
            .evaluate(SampleTime::from_ticks(0), buffers, workspace)
            .unwrap();
        ws281x_parallel::encode(buffers, OUTPUT_PIXELS, ready_buffer.as_mut_slice());
        break 1;
    };

    let mut frames = 0;
    let mut missed = 0;
    let mut evaluation_sum = 0;
    let mut evaluation_max = 0;
    let mut encoding_sum = 0;
    let mut encoding_max = 0;
    let mut wait_sum = 0;
    let mut wait_max = 0;
    let mut total_sum = 0;
    let mut total_max = 0;
    loop {
        let frame_start = Instant::now();
        let mut transfer = match output.send(ready_buffer) {
            Ok(transfer) => transfer,
            Err((error, _, _)) => panic!("I2S DMA start failed: {error:?}"),
        };

        let mut active = playback.lock().await;
        let (sequence, workspace, buffers) = active.as_mut().unwrap();
        let sample_time = sample_time_from_frame(frame_index, OUTPUT_FRAME_RATE).unwrap();
        if sample_time.ticks() >= sequence.signals.duration.ticks() {
            frame_index = 0;
        }
        let sample_time = sample_time_from_frame(frame_index, OUTPUT_FRAME_RATE).unwrap();

        let evaluation_start = Instant::now();
        EVALUATION_TASK.store(
            esp_radio_rtos_driver::current_task().as_ptr() as u32,
            Relaxed,
        );
        sequence.evaluate(sample_time, buffers, workspace).unwrap();
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_us = u32::try_from(evaluation_start.elapsed().as_micros()).unwrap();

        let encoding_start = Instant::now();
        ws281x_parallel::encode(buffers, OUTPUT_PIXELS, spare_buffer.as_mut_slice());
        drop(active);
        let encoding_us = u32::try_from(encoding_start.elapsed().as_micros()).unwrap();

        let wait_start = Instant::now();
        transfer.wait_for_done().await.unwrap();
        let (next_output, finished_buffer) = transfer.wait();
        output = next_output;
        ready_buffer = spare_buffer;
        spare_buffer = finished_buffer;
        let wait_us = u32::try_from(wait_start.elapsed().as_micros()).unwrap();

        let total_us = u32::try_from(frame_start.elapsed().as_micros()).unwrap();
        evaluation_sum += evaluation_us;
        evaluation_max = evaluation_max.max(evaluation_us);
        encoding_sum += encoding_us;
        encoding_max = encoding_max.max(encoding_us);
        wait_sum += wait_us;
        wait_max = wait_max.max(wait_us);
        total_sum += total_us;
        total_max = total_max.max(total_us);
        frames += 1;
        frame_index += 1;

        let frame_period_us = 1_000_000 / OUTPUT_FRAME_RATE;
        if total_us >= frame_period_us {
            missed += 1;
        } else {
            Timer::after_micros(u64::from(frame_period_us - total_us)).await;
        }

        if frames == OUTPUT_FRAME_RATE {
            println!(
                "PLAYBACK core={} frames={} missed={} eval_avg_us={} eval_max_us={} encode_avg_us={} encode_max_us={} dma_wait_avg_us={} dma_wait_max_us={} total_avg_us={} total_max_us={} heap_free={}",
                Cpu::current() as usize,
                frames,
                missed,
                evaluation_sum / frames,
                evaluation_max,
                encoding_sum / frames,
                encoding_max,
                wait_sum / frames,
                wait_max,
                total_sum / frames,
                total_max,
                esp_alloc::HEAP.free()
            );
            frames = 0;
            missed = 0;
            evaluation_sum = 0;
            evaluation_max = 0;
            encoding_sum = 0;
            encoding_max = 0;
            wait_sum = 0;
            wait_max = 0;
            total_sum = 0;
            total_max = 0;
        }
    }
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 96 * 1024);
    let timer = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timer.timer0, p.FROM_CPU_INTR0);

    EVALUATION_TASK.store(
        esp_radio_rtos_driver::current_task().as_ptr() as u32,
        Relaxed,
    );
    drop(core::hint::black_box(Box::new(42u32)));
    EVALUATION_TASK.store(0, Relaxed);
    assert_eq!(EVALUATION_ALLOCATIONS.load(Relaxed), 1);

    // USB serial is provisioning and diagnostics only. The host initiates the
    // handshake, so a damaged boot log cannot be mistaken for a failed boot.
    let mut uart = Uart::new(p.UART0, Config::default())
        .unwrap()
        .with_rx(p.GPIO3)
        .with_tx(p.GPIO1)
        .into_async();
    loop {
        let mut command = [0];
        if uart.read_exact(&mut command).await.is_err() {
            continue;
        }
        match command[0] {
            b'P' => {
                let _ = uart_reply(&mut uart, format_args!("DAWN PROVISION READY")).await;
            }
            b'W' => break,
            _ => {}
        }
    }

    let mut lengths = [0; 2];
    uart.read_exact(&mut lengths).await.unwrap();
    assert!(
        lengths[0] > 0 && lengths[0] <= 32 && lengths[1] <= 64,
        "invalid credential lengths"
    );
    let mut credentials = [0; 96];
    let length = usize::from(lengths[0]) + usize::from(lengths[1]);
    uart.read_exact(&mut credentials[..length]).await.unwrap();
    let ssid = core::str::from_utf8(&credentials[..usize::from(lengths[0])]).unwrap();
    let password = core::str::from_utf8(&credentials[usize::from(lengths[0])..length]).unwrap();
    let config = StationConfig::default()
        .with_ssid(ssid.try_into().unwrap())
        .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
            password.try_into().unwrap(),
        ));
    let interface = wifi::Interface::station();
    let mut controller = wifi::WifiController::new(
        p.WIFI,
        wifi::ControllerConfig::default().with_initial_config(wifi::Config::Station(config)),
    )
    .unwrap();
    credentials.fill(0);
    controller.set_power_saving(PowerSaveMode::None).unwrap();

    let rng = Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    let mut raw_token = [0; 16];
    for word in raw_token.chunks_exact_mut(4) {
        word.copy_from_slice(&rng.random().to_le_bytes());
    }
    let token = token_ascii(raw_token);
    raw_token.fill(0);
    uart.write_all(b"TOKEN ").await.unwrap();
    uart.write_all(&token).await.unwrap();
    uart.write_all(b"\n").await.unwrap();

    let resources = Box::leak(Box::new(StackResources::<3>::new()));
    let (stack, runner) = embassy_net::new(
        interface,
        embassy_net::Config::dhcpv4(Default::default()),
        resources,
        seed,
    );
    spawner.spawn(network(runner).unwrap());
    spawner.spawn(reconnect(controller).unwrap());
    stack.wait_config_up().await;

    let playback: &'static SharedPlayback =
        picoserve::make_static!(SharedPlayback, Mutex::new(None));
    let upload = picoserve::make_static!(UploadGate, Mutex::new(()));
    let app = picoserve::make_static!(
        AppRouter<WebApp>,
        WebApp {
            state: LoaderState {
                playback,
                upload,
                token
            }
        }
        .build_app()
    );
    for task_id in 0..HTTP_WORKERS {
        spawner.spawn(web_server(task_id, stack, app).unwrap());
    }

    #[cfg(feature = "i2s-output")]
    esp_rtos::start_second_core(
        p.CPU_CTRL,
        p.FROM_CPU_INTR1,
        APP_CORE_STACK.init(Stack::new()),
        move || {
            let pins = TxEightBits::new(
                p.GPIO13, p.GPIO18, p.GPIO21, p.GPIO25, NoPin, NoPin, NoPin, NoPin,
            );
            let output = I2sParallel::new(
                p.I2S1,
                p.DMA_I2S1,
                Rate::from_hz(I2S_SAMPLE_RATE),
                pins,
                NoPin,
            )
            .into_async();
            let mut ready_buffer = esp_hal::dma_tx_buffer!(DMA_BYTES).unwrap();
            ready_buffer.as_mut_slice().fill(0);
            ready_buffer.set_length(DMA_BYTES);
            let mut spare_buffer = esp_hal::dma_tx_buffer!(DMA_BYTES).unwrap();
            spare_buffer.as_mut_slice().fill(0);
            spare_buffer.set_length(DMA_BYTES);
            OUTPUT_READY.store(true, Relaxed);
            APP_CORE_EXECUTOR
                .init(esp_rtos::embassy::Executor::new())
                .run(|spawner| {
                    spawner.spawn(
                        render_outputs(playback, output, ready_buffer, spare_buffer).unwrap(),
                    );
                });
        },
    );

    #[cfg(feature = "i2s-output")]
    while !OUTPUT_READY.load(Relaxed) {
        Timer::after_millis(1).await;
    }

    uart_reply(
        &mut uart,
        format_args!(
            "WIFI READY {} {} i2s={} heap_free={}",
            stack.config_v4().unwrap().address.address(),
            HTTP_PORT,
            if cfg!(feature = "i2s-output") {
                "gpio13,18,21,25"
            } else {
                "off"
            },
            esp_alloc::HEAP.free()
        ),
    )
    .await
    .unwrap();

    loop {
        Timer::after_secs(60).await;
    }
}
