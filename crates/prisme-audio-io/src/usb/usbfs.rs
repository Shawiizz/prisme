//! Linux's access to a USB device through its file (`usbfs`): control requests, interface
//! settings, and the isochronous transfers (URBs) submitted and reaped one by one.

use std::ffi::c_void;
use std::io;
use std::os::fd::RawFd;
use std::time::Duration;

const fn ioctl(direction: u64, nr: u64, size: usize) -> u64 {
    (direction << 30) | ((size as u64) << 16) | ((b'U' as u64) << 8) | nr
}
const WRITE: u64 = 1;
const READ: u64 = 2;

#[repr(C)]
struct ControlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout_ms: u32,
    data: *mut c_void,
}

#[repr(C)]
struct SetInterface {
    interface: u32,
    alt: u32,
}

#[repr(C)]
struct UrbHeader {
    kind: u8,
    endpoint: u8,
    status: i32,
    flags: u32,
    buffer: *mut c_void,
    buffer_length: i32,
    actual_length: i32,
    start_frame: i32,
    number_of_packets: i32,
    error_count: i32,
    signal: u32,
    context: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct IsoPacket {
    pub length: u32,
    pub actual_length: u32,
    pub status: u32,
}

const CONTROL: u64 = ioctl(READ | WRITE, 0, size_of::<ControlTransfer>());
const SET_INTERFACE: u64 = ioctl(READ, 4, size_of::<SetInterface>());
const SUBMIT_URB: u64 = ioctl(READ, 10, size_of::<UrbHeader>());
const DISCARD_URB: u64 = ioctl(0, 11, 0);
const REAP_URB: u64 = ioctl(WRITE, 12, size_of::<*mut c_void>());
const GET_SPEED: u64 = ioctl(0, 31, 0);
const URB_TYPE_ISO: u8 = 0;
const URB_ISO_ASAP: u32 = 0x02;
/// `USB_SPEED_HIGH` and above: microframes of 125 µs.
const HIGH_SPEED: i32 = 3;

fn check(result: i32) -> io::Result<i32> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// A control request on the default pipe; returns the bytes moved.
pub(crate) fn control(
    fd: RawFd,
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    data: &mut [u8],
    timeout: Duration,
) -> io::Result<usize> {
    let mut transfer = ControlTransfer {
        request_type,
        request,
        value,
        index,
        length: data.len() as u16,
        timeout_ms: timeout.as_millis() as u32,
        data: data.as_mut_ptr().cast(),
    };
    // SAFETY: the transfer points at `data`, alive and as long as it says, for the call.
    check(unsafe { libc::ioctl(fd, CONTROL as _, &mut transfer) }).map(|n| n as usize)
}

pub(crate) fn set_interface(fd: RawFd, interface: u8, alt: u8) -> io::Result<()> {
    let mut setting = SetInterface { interface: interface.into(), alt: alt.into() };
    // SAFETY: a plain structure the call only reads.
    check(unsafe { libc::ioctl(fd, SET_INTERFACE as _, &mut setting) }).map(drop)
}

/// Whether the device runs at high speed (or above): microframes of 125 µs, else frames of 1 ms.
pub(crate) fn high_speed(fd: RawFd) -> io::Result<bool> {
    // SAFETY: the request takes no argument.
    check(unsafe { libc::ioctl(fd, GET_SPEED as _) }).map(|speed| speed >= HIGH_SPEED)
}

/// An isochronous transfer of `packets` packets on an endpoint, with its buffer. Once
/// submitted it belongs to the kernel until reaped: it must stay where it is until then.
pub(crate) struct Urb {
    /// The header, then its packets, as the kernel reads them.
    memory: Box<[u64]>,
    data: Box<[u8]>,
    packets: usize,
}

impl Urb {
    pub fn new(endpoint: u8, packets: usize, packet_bytes: usize, context: usize) -> Self {
        let bytes = size_of::<UrbHeader>() + packets * size_of::<IsoPacket>();
        let mut urb = Self {
            memory: vec![0u64; bytes.div_ceil(8)].into_boxed_slice(),
            data: vec![0u8; packets * packet_bytes].into_boxed_slice(),
            packets,
        };
        let buffer = urb.data.as_mut_ptr().cast();
        let header = urb.header();
        header.kind = URB_TYPE_ISO;
        header.endpoint = endpoint;
        header.flags = URB_ISO_ASAP;
        header.buffer = buffer;
        header.number_of_packets = packets as i32;
        header.context = context as *mut c_void;
        for p in urb.packets_mut() {
            p.length = packet_bytes as u32;
        }
        urb.header().buffer_length = (packets * packet_bytes) as i32;
        urb
    }

    fn header(&mut self) -> &mut UrbHeader {
        // SAFETY: the memory is as large as a header and aligned for it (u64).
        unsafe { &mut *self.memory.as_mut_ptr().cast::<UrbHeader>() }
    }

    pub fn packets_mut(&mut self) -> &mut [IsoPacket] {
        // SAFETY: the packets follow the header in memory sized for them, aligned for u32.
        unsafe {
            let first = self.memory.as_mut_ptr().cast::<u8>().add(size_of::<UrbHeader>()).cast::<IsoPacket>();
            std::slice::from_raw_parts_mut(first, self.packets)
        }
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// The packets reaped, and the data their bytes went into (each at `packet_bytes` apart).
    pub fn received(&mut self) -> (&[IsoPacket], &[u8]) {
        let packets = self.packets;
        // SAFETY: as in `packets_mut`, read only.
        let list = unsafe {
            let first = self.memory.as_ptr().cast::<u8>().add(size_of::<UrbHeader>()).cast::<IsoPacket>();
            std::slice::from_raw_parts(first, packets)
        };
        (list, &self.data)
    }

    /// Sets the lengths of the packets to send, the data laid end to end.
    pub fn set_lengths(&mut self, lengths: impl Iterator<Item = usize>) {
        let mut total = 0;
        for (p, length) in self.packets_mut().iter_mut().zip(lengths) {
            p.length = length as u32;
            p.actual_length = 0;
            total += length;
        }
        self.header().buffer_length = total as i32;
    }

    /// Where the transfer lies, for [`discard`].
    pub fn address(&mut self) -> usize {
        self.memory.as_mut_ptr() as usize
    }

    pub fn status(&mut self) -> i32 {
        self.header().status
    }

    /// Hands the transfer to the kernel.
    ///
    /// # Safety
    /// The transfer must not move nor be dropped until [`reap`] gave it back.
    pub unsafe fn submit(&mut self, fd: RawFd) -> io::Result<()> {
        let header = self.memory.as_mut_ptr();
        // SAFETY: the header and its packets live as long as the caller keeps them, which it
        // promises until they are reaped.
        check(unsafe { libc::ioctl(fd, SUBMIT_URB as _, header) }).map(drop)
    }
}

/// Takes back the transfer at `address` (from [`Urb::address`]) from the kernel: it comes
/// back through [`reap`], cancelled. One not submitted is left alone.
pub(crate) fn discard(fd: RawFd, address: usize) {
    // SAFETY: the kernel only looks the address up among the transfers it holds.
    unsafe { libc::ioctl(fd, DISCARD_URB as _, address as *mut c_void) };
}

/// Waits for a transfer to complete and gives its context back.
pub(crate) fn reap(fd: RawFd) -> io::Result<usize> {
    let mut done: *mut UrbHeader = std::ptr::null_mut();
    // SAFETY: the kernel writes there the address of a transfer it was given, still alive.
    check(unsafe { libc::ioctl(fd, REAP_URB as _, &mut done) })?;
    // SAFETY: as above: the header of one of our transfers.
    Ok(unsafe { (*done).context } as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_requests_have_the_numbers_of_linux() {
        // as <linux/usbdevice_fs.h> computes them on a 64-bit system
        assert_eq!(CONTROL, 0xc018_5500);
        assert_eq!(SET_INTERFACE, 0x8008_5504);
        assert_eq!(SUBMIT_URB, 0x8038_550a);
        assert_eq!(DISCARD_URB, 0x550b);
        assert_eq!(REAP_URB, 0x4008_550c);
        assert_eq!(GET_SPEED, 0x551f);
    }

    #[test]
    fn a_transfer_lays_its_packets_after_its_header() {
        let mut urb = Urb::new(0x82, 8, 400, 7);
        assert_eq!(size_of::<UrbHeader>(), 56);
        assert!(urb.packets_mut().iter().all(|p| p.length == 400));
        urb.set_lengths([24, 24, 48].into_iter());
        assert_eq!(urb.header().buffer_length, 96);
        assert_eq!(urb.header().context as usize, 7);
    }
}
