//! Real pressure from pointer events and the desktop tablet backends.
//! Mouse input remains at full width; no pressure is inferred from speed.
#[derive(Default)]
pub struct PenInput {
    pressure: Option<f32>,
    eraser: bool,
    touch_until: f64,
    source: Option<&'static str>,
    #[cfg(windows)]
    native: native::Wintab,
    #[cfg(target_os = "linux")]
    x11: x11::Tablet,
}
impl PenInput {
    pub fn update(&mut self, ctx: &eframe::egui::Context) {
        let (contact, touch_sample) = ctx.input(|i| {
            let mut touch_sample = None;
            for event in &i.events {
                if let eframe::egui::Event::Touch { force, phase, .. } = event {
                    match phase {
                        eframe::egui::TouchPhase::Start | eframe::egui::TouchPhase::Move => {
                            if let Some(force) = force.filter(|force| force.is_finite()) {
                                self.pressure = Some(force.clamp(0., 1.));
                                self.touch_until = i.time + 0.2;
                                touch_sample = self.pressure;
                            }
                        }
                        eframe::egui::TouchPhase::End | eframe::egui::TouchPhase::Cancel => {
                            self.touch_until = f64::NEG_INFINITY;
                        }
                    }
                }
            }
            let contact = i.focused && (i.pointer.primary_down() || i.pointer.primary_pressed());
            if !contact || i.time > self.touch_until {
                self.pressure = touch_sample.filter(|_| contact);
            }
            (contact, touch_sample)
        });
        self.source = self.pressure.map(|_| "Pointer events");
        #[cfg(windows)]
        {
            let wintab = self.native.poll();
            let ink = native::take_ink_force();
            self.eraser =
                native::ink_eraser().unwrap_or(self.native.eraser && wintab.is_some()) && contact;
            if contact {
                let pointer = touch_sample.or(self.pressure);
                self.pressure = ink.or(pointer).or(wintab);
                self.source = if ink.is_some() {
                    Some("Windows Ink")
                } else if pointer.is_some() {
                    Some("Pointer events")
                } else {
                    wintab.map(|_| "WinTab")
                };
            }
        }
        #[cfg(target_os = "macos")]
        {
            let (pressure, eraser) = mac::sample();
            self.eraser = eraser && contact;
            if contact && touch_sample.is_none() {
                self.pressure = pressure.or(self.pressure);
                if pressure.is_some() {
                    self.source = Some("AppKit");
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            if !contact {
                self.x11.release();
            } else {
                let pressure = self.x11.pressure();
                if touch_sample.is_none() && self.pressure.is_none() {
                    self.pressure = pressure;
                    self.source = pressure.map(|_| "XInput");
                }
            }
            self.eraser = contact && self.x11.eraser;
        }
    }
    pub(crate) fn source(&self) -> Option<&'static str> {
        self.source
    }

    pub fn eraser(&self) -> bool {
        self.eraser
    }
    #[cfg(test)]
    pub fn test_eraser(&mut self, eraser: bool) {
        self.eraser = eraser;
    }
    pub fn force(&self) -> Option<f32> {
        self.pressure
    }
    pub fn detected(&self) -> bool {
        self.pressure.is_some()
    }
    #[cfg(test)]
    pub fn test_pressure(&mut self, pressure: Option<f32>) {
        self.pressure = pressure;
    }
}
#[cfg(windows)]
pub(crate) fn windows_event_loop<T: 'static>(builder: &mut winit::event_loop::EventLoopBuilder<T>) {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    builder.with_msg_hook(|message| {
        // Observe without consuming messages: winit still delivers position/force.
        unsafe { native::observe_message(message) };
        false
    });
}
#[cfg(windows)]
mod native {
    use std::ffi::{c_char, c_void};
    type Handle = *mut c_void;
    std::thread_local! { static INK_ERASER: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
    std::thread_local! { static INK_FORCE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) }; }
    pub(super) fn take_ink_force() -> Option<f32> {
        INK_FORCE.take()
    }
    pub(super) fn ink_eraser() -> Option<bool> {
        INK_ERASER.get()
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Message {
        window: Handle,
        kind: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: Point,
        private: u32,
    }
    #[repr(C)]
    struct PointerInfo {
        kind: u32,
        id: u32,
        frame: u32,
        flags: u32,
        device: Handle,
        window: Handle,
        pixel: Point,
        himetric: Point,
        pixel_raw: Point,
        himetric_raw: Point,
        time: u32,
        history: u32,
        input: i32,
        keys: u32,
        performance: u64,
        change: u32,
    }
    #[repr(C)]
    struct PenInfo {
        pointer: PointerInfo,
        flags: u32,
        mask: u32,
        pressure: u32,
        rotation: u32,
        tilt_x: i32,
        tilt_y: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetPointerPenInfo(id: u32, info: *mut PenInfo) -> i32;
        fn GetMessageExtraInfo() -> isize;
    }
    pub(super) unsafe fn observe_message(message: *const c_void) {
        // winit provides a valid MSG for the duration of its message hook.
        let message = unsafe { &*message.cast::<Message>() };
        match message.kind {
            0x245 | 0x246 | 0x247 | 0x249 => {
                let mut info = std::mem::MaybeUninit::<PenInfo>::uninit();
                if unsafe { GetPointerPenInfo((message.wparam & 0xffff) as u32, info.as_mut_ptr()) }
                    != 0
                {
                    let info = unsafe { info.assume_init() };
                    INK_ERASER.set(Some(info.flags & 6 != 0));
                    if info.pointer.flags & 4 != 0 && info.mask & 1 != 0 {
                        INK_FORCE.set(Some(info.pressure.min(1024) as f32 / 1024.));
                    }
                }
            }
            0x24a | 0x215 | 0x8 => {
                INK_ERASER.set(None);
                INK_FORCE.set(None);
            }
            // A real mouse must never inherit the previous pen's eraser state.
            0x200 | 0x201
                if (unsafe { GetMessageExtraInfo() } as usize & 0xffffff00) != 0xff515700 =>
            {
                INK_ERASER.set(None)
            }
            _ => {}
        }
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LogContext {
        name: [c_char; 40],
        options: u32,
        status: u32,
        locks: u32,
        msg_base: u32,
        device: u32,
        pkt_rate: u32,
        pkt_data: u32,
        pkt_mode: u32,
        move_mask: u32,
        btn_down: u32,
        btn_up: u32,
        in_org_x: i32,
        in_org_y: i32,
        in_org_z: i32,
        in_ext_x: i32,
        in_ext_y: i32,
        in_ext_z: i32,
        out_org_x: i32,
        out_org_y: i32,
        out_org_z: i32,
        out_ext_x: i32,
        out_ext_y: i32,
        out_ext_z: i32,
        sens_x: u32,
        sens_y: u32,
        sens_z: u32,
        sys_mode: i32,
        sys_org_x: i32,
        sys_org_y: i32,
        sys_ext_x: i32,
        sys_ext_y: i32,
        sys_sens_x: u32,
        sys_sens_y: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Axis {
        min: i32,
        max: i32,
        units: u32,
        resolution: u32,
    }
    type Info = unsafe extern "system" fn(u32, u32, *mut c_void) -> u32;
    type Open = unsafe extern "system" fn(Handle, *mut LogContext, i32) -> Handle;
    type Packets = unsafe extern "system" fn(Handle, i32, *mut c_void) -> i32;
    type Close = unsafe extern "system" fn(Handle) -> i32;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryA(name: *const u8) -> Handle;
        fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: Handle) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> Handle;
        fn GetWindowThreadProcessId(window: Handle, pid: *mut u32) -> u32;
    }
    #[derive(Default)]
    pub struct Wintab {
        attempted: bool,
        module: Handle,
        context: Handle,
        packets: Option<Packets>,
        pub(super) eraser: bool,
        close: Option<Close>,
        min: i32,
        max: i32,
    }
    // Virtual devices (UINT_MAX) do not identify a physical device category.
    fn device_category(device: u32) -> Option<u32> {
        100u32.checked_add(device)
    }

    impl Wintab {
        fn initialize(&mut self) {
            // SAFETY: symbols are checked before conversion; handles are owned
            // by this object and used on the creating GUI thread only.
            unsafe {
                let hwnd = GetForegroundWindow();
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != std::process::id() {
                    return;
                }
                self.attempted = true;
                self.module = LoadLibraryA(c"Wintab32.dll".as_ptr().cast());
                if self.module.is_null() {
                    return;
                }
                let info = GetProcAddress(self.module, c"WTInfoA".as_ptr().cast());
                let open = GetProcAddress(self.module, c"WTOpenA".as_ptr().cast());
                let packets = GetProcAddress(self.module, c"WTPacketsGet".as_ptr().cast());
                let close = GetProcAddress(self.module, c"WTClose".as_ptr().cast());
                if [info, open, packets, close].iter().any(|p| p.is_null()) {
                    return;
                }
                let info: Info = std::mem::transmute(info);
                let open: Open = std::mem::transmute(open);
                let mut lc: LogContext = std::mem::zeroed();
                if info(5, 0, (&mut lc as *mut LogContext).cast()) == 0
                    && info(4, 0, (&mut lc as *mut LogContext).cast()) == 0
                {
                    return;
                }
                let mut axis = Axis::default();
                let mut device_count = 0u32;
                info(1, 4, (&mut device_count as *mut u32).cast());
                // Some drivers expose a virtual default context. In that case,
                // open the first physical device that reports a pressure axis.
                let device = std::iter::once(lc.device)
                    .chain(0..device_count.min(64))
                    .find(|&device| {
                        device_category(device).is_some_and(|category| {
                            info(category, 15, (&mut axis as *mut Axis).cast()) != 0
                                && axis.max > axis.min
                        })
                    });
                let Some(device) = device else {
                    return;
                };
                lc.device = device;
                lc.options |= 1;
                lc.options &= !4;
                lc.pkt_data = 0x422;
                lc.pkt_mode = 0;
                lc.move_mask = 0x422;
                lc.btn_down = 0;
                lc.btn_up = 0;
                self.context = open(hwnd, &mut lc, 1);
                self.min = axis.min;
                self.max = axis.max;
                self.packets = Some(std::mem::transmute(packets));
                self.close = Some(std::mem::transmute(close));
            }
        }
        pub fn poll(&mut self) -> Option<f32> {
            if !self.attempted {
                self.initialize();
            }
            if self.context.is_null() {
                return None;
            }
            if let Some(packets) = self.packets {
                // PK_STATUS, PK_CURSOR, PK_NORMAL_PRESSURE, each one UINT.
                let mut values = [[0u32; 3]; 128];
                let count = unsafe { packets(self.context, 128, values.as_mut_ptr().cast()) }
                    .clamp(0, 128) as usize;
                if count > 0 {
                    let [status, _cursor, pressure] = values[count - 1];
                    // Wintab TPS_INVERT is authoritative, independent of device names.
                    self.eraser = status & 0x10 != 0;
                    let value = (pressure as f32 - self.min as f32)
                        / (self.max as f64 - self.min as f64) as f32;
                    return (value > 0.).then(|| value.clamp(0., 1.));
                }
            }
            None
        }
    }
    #[cfg(test)]
    mod tests {
        use super::device_category;

        #[test]
        fn virtual_and_overflowing_devices_are_rejected() {
            assert_eq!(device_category(0), Some(100));
            assert_eq!(device_category(1), Some(101));
            assert_eq!(device_category(u32::MAX), None);
            assert_eq!(device_category(u32::MAX - 99), None);
        }
    }

    impl Drop for Wintab {
        fn drop(&mut self) {
            unsafe {
                if !self.context.is_null() {
                    if let Some(close) = self.close {
                        close(self.context);
                    }
                }
                if !self.module.is_null() {
                    FreeLibrary(self.module);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void};
    type Object = *mut c_void;
    std::thread_local! {
        static PEN_CONTACT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static FORCE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
        static ERASER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static MONITOR: std::cell::Cell<Object> = const { std::cell::Cell::new(std::ptr::null_mut()) };
    }
    #[repr(C)]
    struct BlockDescriptor {
        reserved: usize,
        size: usize,
    }
    #[repr(C)]
    struct Block {
        isa: *const c_void,
        flags: i32,
        reserved: i32,
        invoke: unsafe extern "C" fn(*const Block, Object) -> Object,
        descriptor: *const BlockDescriptor,
    }
    // A global, capture-free Objective-C block has static lifetime. AppKit retains
    // the event monitor; its callback runs only on the application's UI thread.
    unsafe impl Sync for Block {}
    static DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<Block>(),
    };
    #[link(name = "System")]
    unsafe extern "C" {
        static _NSConcreteGlobalBlock: c_void;
    }
    static BLOCK: Block = Block {
        isa: &raw const _NSConcreteGlobalBlock,
        flags: 1 << 28,
        reserved: 0,
        invoke: observe,
        descriptor: &DESCRIPTOR,
    };
    unsafe extern "C" fn observe(_: *const Block, event: Object) -> Object {
        unsafe {
            let integer: unsafe extern "C" fn(Object, Object) -> usize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let boolean: unsafe extern "C" fn(Object, Object) -> bool =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let subtype: unsafe extern "C" fn(Object, Object) -> i16 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let float: unsafe extern "C" fn(Object, Object) -> f32 =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let kind = integer(event, sel_registerName(c"type".as_ptr()));
            let mouse_subtype = if matches!(kind, 1 | 2 | 5 | 6) {
                subtype(event, sel_registerName(c"subtype".as_ptr()))
            } else {
                0
            };
            match kind {
                _ if kind == 24 || mouse_subtype == 2 => {
                    let entering =
                        boolean(event, sel_registerName(c"isEnteringProximity".as_ptr()));
                    ERASER.set(
                        entering
                            && integer(event, sel_registerName(c"pointingDeviceType".as_ptr()))
                                == 3,
                    );
                    if !entering {
                        PEN_CONTACT.set(false);
                        FORCE.set(None);
                    }
                }
                23 | 1 | 6 if kind == 23 || mouse_subtype == 1 => {
                    let force = float(event, sel_registerName(c"pressure".as_ptr()));
                    if force.is_finite() {
                        FORCE.set(Some(force.clamp(0., 1.)));
                        PEN_CONTACT.set(kind != 23 || force > 0.);
                    }
                }
                1 | 2 => {
                    // Mouse clicks and pen-up must never reuse pressure or the eraser.
                    PEN_CONTACT.set(false);
                    FORCE.set(None);
                }
                _ => (),
            }
        }
        event
    }

    pub(super) fn sample() -> (Option<f32>, bool) {
        if MONITOR.get().is_null() {
            unsafe {
                let install: unsafe extern "C" fn(Object, Object, u64, *const Block) -> Object =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                let mask = (1 << 1) | (1 << 2) | (1 << 5) | (1 << 6) | (1 << 23) | (1 << 24);
                MONITOR.set(install(
                    objc_getClass(c"NSEvent".as_ptr()),
                    sel_registerName(c"addLocalMonitorForEventsMatchingMask:handler:".as_ptr()),
                    mask,
                    &BLOCK,
                ));
            }
        }
        (FORCE.take(), PEN_CONTACT.get() && ERASER.get())
    }

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Object;
        fn sel_registerName(name: *const c_char) -> Object;
        fn objc_msgSend();
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_event_loop<T: 'static>(builder: &mut winit::event_loop::EventLoopBuilder<T>) {
    use winit::platform::{wayland::EventLoopBuilderExtWayland, x11::EventLoopBuilderExtX11};

    match std::env::var("LIBRETOON_LINUX_BACKEND").as_deref() {
        Ok("wayland") => {
            builder.with_wayland();
        }
        Ok("x11") if std::env::var_os("DISPLAY").is_some() => {
            builder.with_x11();
        }
        _ if (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var_os("WAYLAND_SOCKET").is_some())
            && x11::pressure_device_available() =>
        {
            builder.with_x11();
        }
        _ => {}
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::{
        ffi::{CStr, c_char, c_int, c_ulong, c_void},
        ptr,
        time::{Duration, Instant},
    };

    type Handle = *mut c_void;

    #[repr(C)]
    struct Device {
        id: c_int,
        name: *mut c_char,
        usage: c_int,
        attachment: c_int,
        enabled: c_int,
        class_count: c_int,
        classes: *mut *mut c_int,
    }

    #[repr(C)]
    struct Valuator {
        kind: c_int,
        source: c_int,
        number: c_int,
        label: c_ulong,
        min: f64,
        max: f64,
        value: f64,
        resolution: c_int,
        mode: c_int,
    }

    type Query = unsafe extern "C" fn(Handle, c_int, *mut c_int) -> *mut Device;
    type Free = unsafe extern "C" fn(*mut Device);
    type Close = unsafe extern "C" fn(Handle) -> c_int;
    type Property = unsafe extern "C" fn(
        Handle,
        c_int,
        c_ulong,
        std::ffi::c_long,
        std::ffi::c_long,
        c_int,
        c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_ulong,
        *mut c_ulong,
        *mut *mut u8,
    ) -> c_int;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(name: *const c_char, flags: c_int) -> Handle;
        fn dlsym(handle: Handle, name: *const c_char) -> Handle;
        fn dlclose(handle: Handle) -> c_int;
    }

    struct Library(Handle);
    impl Library {
        fn open(name: &CStr) -> Option<Self> {
            let handle = unsafe { dlopen(name.as_ptr(), 2) };
            if handle.is_null() {
                None
            } else {
                Some(Self(handle))
            }
        }

        fn symbol(&self, name: &CStr) -> Option<Handle> {
            let symbol = unsafe { dlsym(self.0, name.as_ptr()) };
            (!symbol.is_null()).then_some(symbol)
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            unsafe { dlclose(self.0) };
        }
    }

    struct Connection {
        display: Handle,
        pressure_atom: c_ulong,
        tool_atom: c_ulong,
        eraser_atom: c_ulong,
        property: Property,
        xfree: unsafe extern "C" fn(Handle) -> c_int,
        tool: std::cell::Cell<Option<(c_int, bool)>>,
        query: Query,
        free: Free,
        close: Close,
        _xi: Library,
        _xlib: Library,
    }

    impl Connection {
        fn open() -> Option<Self> {
            // Load the desktop libraries only when X11 is available, so
            // Wayland-only installations can still use normal pointer input.
            std::env::var_os("DISPLAY")?;
            let xlib = Library::open(c"libX11.so.6")?;
            let xi = Library::open(c"libXi.so.6")?;
            unsafe {
                let open: unsafe extern "C" fn(*const c_char) -> Handle =
                    std::mem::transmute(xlib.symbol(c"XOpenDisplay")?);
                let close: Close = std::mem::transmute(xlib.symbol(c"XCloseDisplay")?);
                let intern: unsafe extern "C" fn(Handle, *const c_char, c_int) -> c_ulong =
                    std::mem::transmute(xlib.symbol(c"XInternAtom")?);
                let version: unsafe extern "C" fn(Handle, *mut c_int, *mut c_int) -> c_int =
                    std::mem::transmute(xi.symbol(c"XIQueryVersion")?);
                let query: Query = std::mem::transmute(xi.symbol(c"XIQueryDevice")?);
                let free: Free = std::mem::transmute(xi.symbol(c"XIFreeDeviceInfo")?);
                let property: Property = std::mem::transmute(xi.symbol(c"XIGetProperty")?);
                let xfree = std::mem::transmute(xlib.symbol(c"XFree")?);
                let display = open(ptr::null());
                if display.is_null() {
                    return None;
                }
                let (mut major, mut minor) = (2, 0);
                if version(display, &mut major, &mut minor) != 0 || major < 2 {
                    close(display);
                    return None;
                }
                let pressure_atom = intern(display, c"Abs Pressure".as_ptr(), 0);
                Some(Self {
                    display,
                    pressure_atom,
                    tool_atom: intern(display, c"Wacom Tool Type".as_ptr(), 0),
                    eraser_atom: intern(display, c"ERASER".as_ptr(), 0),
                    property,
                    xfree,
                    tool: std::cell::Cell::new(None),
                    query,
                    free,
                    close,
                    _xi: xi,
                    _xlib: xlib,
                })
            }
        }

        fn eraser(&self, device: &Device) -> bool {
            if let Some((id, eraser)) = self.tool.get().filter(|(id, _)| *id == device.id) {
                let _ = id;
                return eraser;
            }
            // Wacom publishes a typed atom even when the user renames a device.
            // Other XI2 drivers commonly expose a separate eraser-named device.
            let mut eraser = unsafe {
                !device.name.is_null()
                    && CStr::from_ptr(device.name)
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("eraser")
            };
            unsafe {
                let (mut kind, mut format, mut count, mut after) = (0, 0, 0, 0);
                let mut data = ptr::null_mut();
                if (self.property)(
                    self.display,
                    device.id,
                    self.tool_atom,
                    0,
                    1,
                    0,
                    0,
                    &mut kind,
                    &mut format,
                    &mut count,
                    &mut after,
                    &mut data,
                ) == 0
                    && kind == 4
                    && format == 32
                    && count == 1
                    && !data.is_null()
                {
                    eraser = *data.cast::<c_ulong>() == self.eraser_atom;
                }
                if !data.is_null() {
                    (self.xfree)(data.cast());
                }
            }
            self.tool.set(Some((device.id, eraser)));
            eraser
        }

        fn sample(&self) -> (bool, Option<f32>, bool) {
            // Query all devices because a tablet may be plugged in mid-session.
            // Inspect only enabled slave pointers with the standard pressure axis.
            unsafe {
                let mut count = 0;
                let devices = (self.query)(self.display, 0, &mut count);
                if devices.is_null() {
                    return (false, None, false);
                }
                let mut found = false;
                let mut pressure = None;
                let mut eraser = false;
                for device in std::slice::from_raw_parts(devices, count.max(0) as usize) {
                    if device.enabled == 0 || device.usage != 3 || device.classes.is_null() {
                        continue;
                    }
                    for &class in std::slice::from_raw_parts(
                        device.classes,
                        device.class_count.max(0) as usize,
                    ) {
                        if class.is_null() || *class != 2 {
                            continue;
                        }
                        let axis = &*class.cast::<Valuator>();
                        if axis.label == self.pressure_atom && axis.mode == 1 && axis.max > axis.min
                        {
                            found = true;
                            let sample = ((axis.value - axis.min) / (axis.max - axis.min)) as f32;
                            if sample.is_finite() && sample > 0. {
                                pressure = Some(sample.clamp(0., 1.));
                                eraser = self.eraser(device);
                                break;
                            }
                        }
                    }
                    if pressure.is_some() {
                        break;
                    }
                }
                (self.free)(devices);
                (found, pressure, eraser)
            }
        }
    }

    impl Drop for Connection {
        fn drop(&mut self) {
            unsafe { (self.close)(self.display) };
        }
    }

    pub(super) fn pressure_device_available() -> bool {
        Connection::open().is_some_and(|connection| connection.sample().0)
    }

    #[derive(Default)]
    pub(super) struct Tablet {
        attempted: bool,
        connection: Option<Connection>,
        next_sample: Option<Instant>,
        last: Option<f32>,
        pub(super) eraser: bool,
    }

    impl Tablet {
        pub(super) fn pressure(&mut self) -> Option<f32> {
            let now = Instant::now();
            if self.next_sample.is_some_and(|next| now < next) {
                return self.last;
            }
            if !self.attempted {
                self.attempted = true;
                self.connection = Connection::open();
            }
            let (device, pressure, eraser) = self.connection.as_ref()?.sample();
            self.eraser = eraser;
            self.next_sample = Some(
                now + if device {
                    Duration::from_millis(8)
                } else {
                    Duration::from_secs(2)
                },
            );
            self.last = pressure;
            pressure
        }

        pub(super) fn release(&mut self) {
            self.eraser = false;
            self.last = None;
            self.next_sample = None;
        }
    }
}
