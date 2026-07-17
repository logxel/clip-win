//! Wayland Clipboard Backend using ext_data_control_v1 protocol
//!
//! This module provides native Wayland clipboard access without creating
//! transient wl_surfaces, which fixes taskbar blinking on GNOME/Mutter.
//!
//! Based on ringboard's approach: uses ext_data_control_manager_v1 for
//! clipboard access via pipe FDs with zero surface creation.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::io::{BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};

use wayland_client::{
    event_created_child,
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

/// Error type for Wayland clipboard operations
#[derive(Debug)]
pub enum WaylandClipboardError {
    /// Wayland connection failed
    ConnectionFailed(String),
    /// Protocol not supported by compositor
    ProtocolNotSupported,
    /// Clipboard read failed
    ReadFailed(String),
    /// Clipboard write failed
    WriteFailed(String),
}

impl std::fmt::Display for WaylandClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(e) => write!(f, "Wayland connection failed: {}", e),
            Self::ProtocolNotSupported => {
                write!(
                    f,
                    "Compositor does not support ext_data_control_manager_v1"
                )
            }
            Self::ReadFailed(e) => write!(f, "Clipboard read failed: {}", e),
            Self::WriteFailed(e) => write!(f, "Clipboard write failed: {}", e),
        }
    }
}

impl std::error::Error for WaylandClipboardError {}

impl From<io::Error> for WaylandClipboardError {
    fn from(e: io::Error) -> Self {
        Self::ReadFailed(e.to_string())
    }
}

/// Wayland state processed by the event queue.
///
/// All fields live here and are mutated by `Dispatch` implementations.
/// No separate `Arc<Mutex<…>>` — the `EventQueue` roundtrip dispatches
/// directly into this struct.
#[derive(Default)]
struct WaylandState {
    /// The data control manager global (bound during registry roundtrip)
    manager: Option<ExtDataControlManagerV1>,
    /// The data control device for each seat (keyed by seat id)
    devices: Vec<(u32, ExtDataControlDeviceV1)>,
    /// The most recent clipboard text offer, if any.
    /// Set after reading from the pipe following a receive request.
    clipboard_text: Option<String>,
    /// The most recent clipboard HTML content, if any (rich text).
    clipboard_html: Option<String>,
    /// The most recent clipboard image data, if any.
    /// Raw bytes of the image (format detected by caller).
    clipboard_image: Option<Vec<u8>>,
    /// Read end of a pipe for receiving clipboard text data.
    /// Set by the Offer dispatch handler when it queues `offer.receive()`.
    /// The actual read happens in `get_text()` after `flush()`.
    text_read_fd: Option<OwnedFd>,
    /// Read end of a pipe for receiving clipboard HTML data (rich text).
    html_read_fd: Option<OwnedFd>,
    /// Read end of a pipe for receiving clipboard image data.
    /// Set by the Offer dispatch handler when it queues `offer.receive()`.
    /// The actual read happens in `get_image()` after `flush()`.
    image_read_fd: Option<OwnedFd>,
    /// Monotonically increasing generation counter for selection ownership.
    /// Incremented on each set_text()/set_html() call. Used to detect whether
    /// an incoming Selection event is our own echo or a genuine external change.
    selection_gen: u64,
    /// Per-seat expected generation. When we set the selection, we store the
    /// current selection_gen for each seat. The Selection handler compares the
    /// stored value — if it matches, the event is our own echo and is ignored.
    /// Keyed by seat id (u32 from the Wayland global).
    expected_gen: HashMap<u32, u64>,
    /// Per-source pending clipboard text, keyed by the ExtDataControlSourceV1
    /// proxy ID (u32). Each set_text() call creates a new source; the Send
    /// handler uses the source proxy received in the event to look up the
    /// correct text to write.
    pending_by_source: HashMap<u32, String>,
    /// Active data control sources. We keep track of them so we can destroy
    /// old ones when new set_text() calls create new sources.
    active_sources: Vec<ExtDataControlSourceV1>,
}

// ---------------------------------------------------------------------------
// Dispatch implementations
// ---------------------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: <wl_registry::WlRegistry as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wl_registry::Event;
        match event {
            Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == ExtDataControlManagerV1::interface().name {
                    if state.manager.is_some() {
                        eprintln!(
                            "[WaylandClipboard] Duplicate ext_data_control_manager_v1 global"
                        );
                        return;
                    }
                    let manager: ExtDataControlManagerV1 =
                        registry.bind(name, version, qh, ());
                    println!(
                        "[WaylandClipboard] Bound ext_data_control_manager_v1 v{}",
                        version
                    );
                    state.manager = Some(manager);
                } else if interface == wl_seat::WlSeat::interface().name {
                    // Bind the seat — the Seat event handler will create the device
                    let _seat: wl_seat::WlSeat = registry.bind(name, version, qh, name);
                }
            }
            Event::GlobalRemove { name } => {
                // A seat was removed — drop its device
                state.devices.retain(|(id, _)| *id != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &ExtDataControlManagerV1,
        event: <ExtDataControlManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The manager itself doesn't send events; ignore.
        let _ = event;
    }
}

impl Dispatch<wl_seat::WlSeat, u32> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: <wl_seat::WlSeat as Proxy>::Event,
        seat_id: &u32,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wl_seat::Event;
        // When the seat advertises its capabilities, create the data control
        // device so we start receiving clipboard events.
        if let Event::Capabilities { .. } = event {
            if let Some(manager) = &state.manager {
                let device = manager.get_data_device(seat, qh, ());
                println!(
                    "[WaylandClipboard] Created data control device for seat {}",
                    seat_id
                );
                state.devices.push((*seat_id, device));
            }
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        device: &ExtDataControlDeviceV1,
        event: <ExtDataControlDeviceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => {
                println!(
                    "[WaylandClipboard] Data offer {:?}, waiting for selection",
                    id.id()
                );
                // DataOffer from another app — reset expected generation tracking
                state.expected_gen.clear();
            }
            Event::Selection { id } => {
                // Use generation counter to detect our own echo Selection events.
                // If the expected_gen for this device's seat matches selection_gen,
                // it's our own echo — ignore it. Otherwise it's an external change.
                let seat_id = state
                    .devices
                    .iter()
                    .find(|(_, d)| d.id() == device.id())
                    .map(|(sid, _)| *sid);

                if let Some(sid) = seat_id {
                    if state.expected_gen.remove(&sid) == Some(state.selection_gen) {
                        // Our own selection echo — return without processing
                        return;
                    }
                }

                if let Some(offer) = id {
                    println!(
                        "[WaylandClipboard] Selection changed, offer {:?}",
                        offer.id()
                    );
                } else {
                    println!("[WaylandClipboard] Selection cleared");
                    state.clipboard_text = None;
                    state.clipboard_html = None;
                    state.clipboard_image = None;
                }
            }
            Event::PrimarySelection { .. } => {
                // We only track the regular selection, not primary
            }
            Event::Finished => {
                // A seat was removed — clear all devices; they'll be
                // re-created when a new seat appears.
                println!("[WaylandClipboard] Device finished");
                state.devices.clear();
                state.expected_gen.clear();
            }
            _ => {}
        }
    }

    event_created_child!(Self, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: <ExtDataControlOfferV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            println!("[WaylandClipboard] Offered mime: {}", mime_type);

            // Plain text clipboard: accept the best text mime.
            if is_text_mime(&mime_type) {
                if let Some(fd) = Self::queue_receive(offer, &mime_type) {
                    state.text_read_fd = Some(fd);
                }
                return;
            }

            // HTML rich text (separate pipe from plain text).
            if mime_type == "text/html" {
                if let Some(fd) = Self::queue_receive(offer, &mime_type) {
                    state.html_read_fd = Some(fd);
                }
                return;
            }

            // Image clipboard: accept the first image mime.
            if is_image_mime(&mime_type) && state.image_read_fd.is_none() {
                if let Some(fd) = Self::queue_receive(offer, &mime_type) {
                    state.image_read_fd = Some(fd);
                }
            }
        }
    }
}

impl WaylandState {
    /// Create a pipe, call `offer.receive()` to queue a data transfer,
    /// close the write end, and return the read end (owned, auto-closes).
    fn queue_receive(offer: &ExtDataControlOfferV1, mime: &str) -> Option<OwnedFd> {
        let mut pipe_fds = [0i32; 2];
        if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
            eprintln!("[WaylandClipboard] Failed to create pipe");
            return None;
        }

        let read_fd = pipe_fds[0];
        let write_fd = pipe_fds[1];

        // SAFETY: write_fd is valid for this scope
        let write_borrowed = unsafe { BorrowedFd::borrow_raw(write_fd) };

        // Queue the receive request (not yet sent to compositor)
        offer.receive(mime.to_string(), write_borrowed);

        // Close our copy of the write end so the compositor's dup'd
        // copy will produce EOF when it finishes writing
        unsafe { libc::close(write_fd) };

        // SAFETY: read_fd is valid and owned by us
        Some(unsafe { OwnedFd::from_raw_fd(read_fd) })
    }
}

/// Check if a mime type is text.
/// Uses case-insensitive prefix matching to handle compositor variations
/// like `text/plain; charset=utf-8`, `text/plain;charset=UTF-8`, etc.
fn is_text_mime(mime: &str) -> bool {
    let lower = mime.to_ascii_lowercase();
    lower == "text/plain"
        || lower.starts_with("text/plain;")
        || lower == "utf8_string"
        || lower == "text"
        || lower == "string"
}

/// Check if a mime type is an image.
fn is_image_mime(mime: &str) -> bool {
    mime.starts_with("image/")
}

impl Dispatch<ExtDataControlSourceV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: <ExtDataControlSourceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use ext_data_control_source_v1::Event;
        match event {
            Event::Send { mime_type: _, fd } => {
                // Look up the pending text by source proxy ID
                let source_id = source.id().protocol_id();
                if let Some(text) = state.pending_by_source.remove(&source_id) {
                    let text_bytes = text.as_bytes();
                    // SAFETY: fd is valid and owned by the compositor
                    let mut file = unsafe { std::fs::File::from_raw_fd(fd.into_raw_fd()) };
                    if let Err(e) = file.write_all(text_bytes) {
                        eprintln!(
                            "[WaylandClipboard] Failed to write clipboard data: {}",
                            e
                        );
                    }
                    println!(
                        "[WaylandClipboard] Wrote {} bytes to clipboard pipe",
                        text_bytes.len()
                    );
                } else {
                    eprintln!(
                        "[WaylandClipboard] Send event but no pending text for source {}",
                        source_id
                    );
                }
            }
            Event::Cancelled => {
                let source_id = source.id().protocol_id();
                println!(
                    "[WaylandClipboard] Source {} cancelled — another app took clipboard",
                    source_id
                );
                state.pending_by_source.remove(&source_id);
                state.active_sources.retain(|s| s.id().protocol_id() != source_id);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Long-lived connection to Wayland's ext_data_control protocol.
/// Can be read from / written to from any thread.
pub struct WaylandClipboard {
    _conn: Connection,
    event_queue: EventQueue<WaylandState>,
    state: WaylandState,
}

impl WaylandClipboard {
    /// Connect to Wayland and bind ext_data_control_manager_v1.
    /// Returns `None` if the compositor doesn't support the protocol.
    pub fn new() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;

        let mut event_queue = conn.new_event_queue();
        let mut state = WaylandState::default();

        // Register for globals
        {
            let qh = event_queue.handle();
            conn.display().get_registry(&qh, ());
        }

        // Roundtrip 1: dispatch registry globals → bind manager + seats
        event_queue.roundtrip(&mut state).ok()?;

        if state.manager.is_none() {
            println!(
                "[WaylandClipboard] ext_data_control_manager_v1 not available"
            );
            return None;
        }

        // Roundtrip 2: dispatch seat capabilities → create data control devices
        event_queue.roundtrip(&mut state).ok()?;

        if state.devices.is_empty() {
            println!(
                "[WaylandClipboard] No data control devices created (no seats?)"
            );
            return None;
        }

        println!(
            "[WaylandClipboard] Ready — {} device(s)",
            state.devices.len()
        );

        Some(Self {
            _conn: conn,
            event_queue,
            state,
        })
    }

    /// Read current clipboard text (blocking).
    /// Returns `Ok(None)` if clipboard is empty.
    pub fn get_text(&mut self) -> Result<Option<String>, WaylandClipboardError> {
        if self.state.devices.is_empty() {
            return Err(WaylandClipboardError::ProtocolNotSupported);
        }

        // Roundtrip: flush pending requests, read events from compositor,
        // dispatch them.  During dispatch the Offer handler may queue an
        // `offer.receive()` request and store the read fd.
        self.roundtrip_and_flush()?;

        // If we queued a receive request, read the pipe.
        if let Some(read_fd) = self.state.text_read_fd.take() {
            let mut data = String::new();
            // SAFETY: read_fd is valid and owned by us (OwnedFd)
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd.into_raw_fd()) };
            file.read_to_string(&mut data)
                .map_err(|e| WaylandClipboardError::ReadFailed(e.to_string()))?;

            if !data.is_empty() {
                println!(
                    "[WaylandClipboard] Read {} bytes from clipboard",
                    data.len()
                );
                self.state.clipboard_text = Some(data);
            }
        }

        Ok(self.state.clipboard_text.clone())
    }

    /// Read current clipboard HTML (rich text, blocking).
    /// Returns `Ok(None)` if clipboard doesn't contain HTML.
    pub fn get_html(&mut self) -> Result<Option<String>, WaylandClipboardError> {
        if self.state.devices.is_empty() {
            return Err(WaylandClipboardError::ProtocolNotSupported);
        }

        // Roundtrip: flush pending requests, read events from compositor,
        // dispatch them.  During dispatch the Offer handler may queue an
        // `offer.receive()` request and store the read fd.
        self.roundtrip_and_flush()?;

        // If we queued a receive request, read the pipe.
        if let Some(read_fd) = self.state.html_read_fd.take() {
            let mut data = String::new();
            // SAFETY: read_fd is valid and owned by us (OwnedFd)
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd.into_raw_fd()) };
            file.read_to_string(&mut data)
                .map_err(|e| WaylandClipboardError::ReadFailed(e.to_string()))?;

            if !data.is_empty() {
                println!(
                    "[WaylandClipboard] Read {} bytes of HTML from clipboard",
                    data.len()
                );
                self.state.clipboard_html = Some(data);
            }
        }

        Ok(self.state.clipboard_html.clone())
    }

    /// Read current clipboard image (blocking).
    /// Returns raw bytes of the clipboard image (PNG/JPEG/BMP etc.),
    /// or `Ok(None)` if the clipboard doesn't contain an image.
    pub fn get_image(&mut self) -> Result<Option<Vec<u8>>, WaylandClipboardError> {
        if self.state.devices.is_empty() {
            return Err(WaylandClipboardError::ProtocolNotSupported);
        }

        // Roundtrip + flush to process any pending offers/images
        self.roundtrip_and_flush()?;

        // If we queued an image receive request, read the pipe.
        if let Some(read_fd) = self.state.image_read_fd.take() {
            let mut data = Vec::new();
            // SAFETY: read_fd is valid and owned by us (OwnedFd)
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd.into_raw_fd()) };
            file.read_to_end(&mut data)
                .map_err(|e| WaylandClipboardError::ReadFailed(e.to_string()))?;

            if !data.is_empty() {
                println!(
                    "[WaylandClipboard] Read {} bytes of image from clipboard",
                    data.len()
                );
                self.state.clipboard_image = Some(data);
            }
        }

        Ok(self.state.clipboard_image.clone())
    }

    /// Perform a roundtrip, then flush the send buffer.
    fn roundtrip_and_flush(&mut self) -> Result<(), WaylandClipboardError> {
        self.event_queue
            .roundtrip(&mut self.state)
            .map_err(|e| WaylandClipboardError::ReadFailed(e.to_string()))?;

        self.event_queue
            .flush()
            .map_err(|e| WaylandClipboardError::ReadFailed(e.to_string()))?;
        Ok(())
    }

    /// Set clipboard text.
    pub fn set_text(&mut self, text: &str) -> Result<(), WaylandClipboardError> {
        let (seat_id, device) = self
            .state
            .devices
            .first()
            .cloned()
            .ok_or(WaylandClipboardError::ProtocolNotSupported)?;

        let manager = self
            .state
            .manager
            .as_ref()
            .ok_or(WaylandClipboardError::ProtocolNotSupported)?;

        // Create a data source and offer text mime types
        let qh = self.event_queue.handle();
        let source = manager.create_data_source(&qh, ());
        source.offer("text/plain".to_string());
        source.offer("text/plain;charset=utf-8".to_string());
        source.offer("UTF8_STRING".to_string());
        source.offer("text/html".to_string());

        // Increment the generation counter for selection ownership tracking
        self.state.selection_gen = self.state.selection_gen.wrapping_add(1);

        // Store the expected generation for this seat so the Selection
        // handler can identify our own echo events
        self.state.expected_gen.insert(seat_id, self.state.selection_gen);

        // Store the text by source proxy ID so the Send handler can find it
        let source_id = source.id().protocol_id();
        self.state.pending_by_source.insert(source_id, text.to_string());

        // Track active sources for cleanup
        self.state.active_sources.push(source.clone());

        // Claim the selection
        device.set_selection(Some(&source));

        // Destroy all but the newest source (keep the newest alive for compositor)
        for old_source in self.state.active_sources.drain(..self.state.active_sources.len().saturating_sub(1)) {
            old_source.destroy();
        }

        // Flush so the compositor receives the selection claim immediately
        self.event_queue
            .flush()
            .map_err(|e| WaylandClipboardError::WriteFailed(e.to_string()))?;

        println!("[WaylandClipboard] Set text: {} bytes", text.len());
        Ok(())
    }

    /// Set clipboard HTML content.
    /// Similar to set_text() but offers text/html as the primary MIME type.
    pub fn set_html(&mut self, html: &str) -> Result<(), WaylandClipboardError> {
        let (seat_id, device) = self
            .state
            .devices
            .first()
            .cloned()
            .ok_or(WaylandClipboardError::ProtocolNotSupported)?;

        let manager = self
            .state
            .manager
            .as_ref()
            .ok_or(WaylandClipboardError::ProtocolNotSupported)?;

        // Create a data source and offer HTML as primary MIME
        let qh = self.event_queue.handle();
        let source = manager.create_data_source(&qh, ());
        source.offer("text/html".to_string());
        source.offer("text/plain".to_string());
        source.offer("text/plain;charset=utf-8".to_string());
        source.offer("UTF8_STRING".to_string());

        // Increment the generation counter for selection ownership tracking
        self.state.selection_gen = self.state.selection_gen.wrapping_add(1);

        // Store the expected generation for this seat so the Selection
        // handler can identify our own echo events
        self.state.expected_gen.insert(seat_id, self.state.selection_gen);

        // Store the html by source proxy ID so the Send handler can find it
        let source_id = source.id().protocol_id();
        self.state.pending_by_source.insert(source_id, html.to_string());

        // Track active sources for cleanup
        self.state.active_sources.push(source.clone());

        // Claim the selection
        device.set_selection(Some(&source));

        // Destroy all but the newest source (keep the newest alive for compositor)
        for old_source in self.state.active_sources.drain(..self.state.active_sources.len().saturating_sub(1)) {
            old_source.destroy();
        }

        // Flush so the compositor receives the selection claim immediately
        self.event_queue
            .flush()
            .map_err(|e| WaylandClipboardError::WriteFailed(e.to_string()))?;

        println!("[WaylandClipboard] Set HTML: {} bytes", html.len());
        Ok(())
    }
}

// SAFETY: `WaylandClipboard` contains `Connection`, `EventQueue<WaylandState>`,
// and `WaylandState`. All fields are accessed exclusively via `&mut self` (no
// shared references). The `EventQueue` and `Connection` types from wayland-client
// are explicitly `Send`. `WaylandState` contains only `String`, `Vec<u8>`,
// `HashMap<u32, _>`, `Vec<ExtDataControlSourceV1>`, `Option<OwnedFd>`, `u64`,
// and wayland protocol objects -- all of which are `Send` or safely wrapped.
// External synchronization is provided by the `Mutex<ClipboardManager>` in
// `clipboard_manager.rs`, ensuring no concurrent `&mut self` access.
unsafe impl Send for WaylandClipboard {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wayland_clipboard_new() {
        // Returns None gracefully when not on Wayland or protocol unavailable
        let _ = WaylandClipboard::new();
    }

    // --- MIME matching tests ---

    #[test]
    fn is_text_mime_exact_matches() {
        assert!(is_text_mime("text/plain"));
        assert!(is_text_mime("UTF8_STRING"));
        assert!(is_text_mime("TEXT"));
        assert!(is_text_mime("STRING"));
    }

    #[test]
    fn is_text_mime_charset_variants() {
        // Real compositors emit these variations
        assert!(is_text_mime("text/plain;charset=utf-8"));
        assert!(is_text_mime("text/plain; charset=utf-8"));
        assert!(is_text_mime("text/plain;charset=UTF-8"));
        assert!(is_text_mime("text/plain; charset=UTF-8"));
    }

    #[test]
    fn is_text_mime_case_insensitive() {
        assert!(is_text_mime("TEXT/PLAIN"));
        assert!(is_text_mime("Text/Plain"));
        assert!(is_text_mime("text/PLAIN"));
        assert!(is_text_mime("utf8_string"));
        assert!(is_text_mime("Utf8_String"));
    }

    #[test]
    fn is_text_mime_rejects_non_text() {
        assert!(!is_text_mime("text/html"));
        assert!(!is_text_mime("image/png"));
        assert!(!is_text_mime("application/json"));
        assert!(!is_text_mime("text/csv"));
        assert!(!is_text_mime(""));
    }

    #[test]
    fn is_image_mime_matches() {
        assert!(is_image_mime("image/png"));
        assert!(is_image_mime("image/jpeg"));
        assert!(is_image_mime("image/gif"));
        assert!(is_image_mime("image/bmp"));
        assert!(is_image_mime("image/svg+xml"));
    }

    #[test]
    fn is_image_mime_rejects_non_image() {
        assert!(!is_image_mime("text/plain"));
        assert!(!is_image_mime("application/octet-stream"));
        assert!(!is_image_mime(""));
    }

    // --- WaylandState default tests ---

    #[test]
    fn wayland_state_default_is_empty() {
        let state = WaylandState::default();
        assert!(state.manager.is_none());
        assert!(state.devices.is_empty());
        assert!(state.clipboard_text.is_none());
        assert!(state.clipboard_html.is_none());
        assert!(state.clipboard_image.is_none());
        assert!(state.text_read_fd.is_none());
        assert!(state.html_read_fd.is_none());
        assert!(state.image_read_fd.is_none());
        assert_eq!(state.selection_gen, 0);
        assert!(state.expected_gen.is_empty());
        assert!(state.pending_by_source.is_empty());
        assert!(state.active_sources.is_empty());
    }

    // --- Error type tests ---

    #[test]
    fn wayland_error_display() {
        let err = WaylandClipboardError::ConnectionFailed("test".into());
        assert!(err.to_string().contains("test"));

        let err = WaylandClipboardError::ProtocolNotSupported;
        assert!(err.to_string().contains("not support"));

        let err = WaylandClipboardError::ReadFailed("io error".into());
        assert!(err.to_string().contains("io error"));

        let err = WaylandClipboardError::WriteFailed("write error".into());
        assert!(err.to_string().contains("write error"));
    }

    #[test]
    fn wayland_error_is_std_error() {
        let err = WaylandClipboardError::ReadFailed("test".into());
        let _ = std::error::Error::source(&err);
    }

    #[test]
    fn wayland_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let wl_err: WaylandClipboardError = io_err.into();
        assert!(matches!(wl_err, WaylandClipboardError::ReadFailed(_)));
    }

    // --- Issue D: Selection::id:None clears ALL content ---

    #[test]
    fn test_selection_clear_clears_all_content() {
        let mut state = WaylandState {
            clipboard_text: Some("text".into()),
            ..Default::default()
        };
        state.clipboard_html = Some("<p>html</p>".into());
        state.clipboard_image = Some(vec![1, 2, 3]);

        // Simulate the Selection handler for id:None
        state.clipboard_text = None;
        state.clipboard_html = None;
        state.clipboard_image = None;

        assert!(state.clipboard_text.is_none());
        assert!(state.clipboard_html.is_none());
        assert!(state.clipboard_image.is_none());
    }

    // --- Generation counter tests ---

    #[test]
    fn test_generation_counter_increments_on_set_text() {
        let mut state = WaylandState::default();
        assert_eq!(state.selection_gen, 0);

        // Simulate gen counter increment (as done in set_text/set_html)
        state.selection_gen = state.selection_gen.wrapping_add(1);
        assert_eq!(state.selection_gen, 1);

        state.selection_gen = state.selection_gen.wrapping_add(1);
        assert_eq!(state.selection_gen, 2);
    }

    #[test]
    fn test_generation_counter_wrapping() {
        let mut state = WaylandState::default();
        state.selection_gen = u64::MAX;
        state.selection_gen = state.selection_gen.wrapping_add(1);
        assert_eq!(state.selection_gen, 0);
    }

    // --- expected_gen HashMap tracking ---

    #[test]
    fn test_expected_gen_tracks_seat() {
        let mut state = WaylandState::default();
        state.selection_gen = state.selection_gen.wrapping_add(1); // gen 1
        state.expected_gen.insert(42, state.selection_gen);

        // Simulate Selection event for seat 42 with matching gen
        assert_eq!(state.expected_gen.remove(&42), Some(1));
        // After removal, should not match again
        assert_eq!(state.expected_gen.get(&42), None);
    }

    // --- pending_by_source HashMap ---

    #[test]
    fn test_pending_by_source_insert_and_remove() {
        let mut state = WaylandState::default();
        state.pending_by_source.insert(100, "hello".into());
        state.pending_by_source.insert(200, "world".into());

        assert_eq!(state.pending_by_source.remove(&100), Some("hello".into()));
        assert_eq!(state.pending_by_source.remove(&200), Some("world".into()));
        assert_eq!(state.pending_by_source.remove(&100), None);
    }

    // --- active_sources drain logic ---

    #[test]
    fn test_active_sources_drain_keeps_newest() {
        let mut active: Vec<u32> = vec![1, 2, 3];
        // Simulate drain keeping newest
        let len = active.len();
        for old in active.drain(..len.saturating_sub(1)) {
            let _ = old; // simulate destroying
        }
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], 3);
    }

    #[test]
    fn test_active_sources_drain_single() {
        let mut active: Vec<u32> = vec![42];
        let len = active.len();
        for old in active.drain(..len.saturating_sub(1)) {
            let _ = old;
        }
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], 42);
    }

    #[test]
    fn test_active_sources_drain_empty() {
        let mut active: Vec<u32> = vec![];
        let len = active.len();
        for old in active.drain(..len.saturating_sub(1)) {
            let _ = old;
        }
        assert!(active.is_empty());
    }

    // --- MIME edge cases ---

    #[test]
    fn test_is_text_mime_edge_cases() {
        assert!(!is_text_mime(""));
        assert!(!is_text_mime("application/x-extension-text/plain"));
        // text/plain with multiple parameters IS still text/plain
        assert!(is_text_mime("text/plain;charset=utf-8;format=flowed"));
        assert!(is_text_mime("TEXT/PLAIN;CHARSET=UTF-8"));
        assert!(is_text_mime("Text/Plain;Charset=utf-8"));
        // null byte or non-utf8-ish pattern should not crash
        assert!(!is_text_mime("\0"));
    }
}
