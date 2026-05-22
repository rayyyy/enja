#![allow(non_snake_case, non_upper_case_globals)]

use std::collections::VecDeque;
use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr;
use std::sync::{Arc, Mutex};

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFMutableDictionary};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::dictionary::CFDictionaryRef;

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

type AudioObjectID = u32;
type OSStatus = i32;
type AudioDeviceIOProcID = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    number_buffers: u32,
    buffers: [AudioBuffer; 1],
}

#[repr(C)]
struct SMPTETime {
    counter: i64,
    type_: u32,
    flags: u32,
    hours: i16,
    minutes: i16,
    seconds: i16,
    frames: i16,
}

#[repr(C)]
struct AudioTimeStamp {
    sample_time: f64,
    host_time: u64,
    rate_scalar: f64,
    word_clock_time: u64,
    smpte_time: SMPTETime,
    flags: u32,
    reserved: u32,
}

type AudioDeviceIOProc = unsafe extern "C" fn(
    in_device: AudioObjectID,
    in_now: *const AudioTimeStamp,
    in_input_data: *const AudioBufferList,
    in_input_time: *const AudioTimeStamp,
    out_output_data: *mut AudioBufferList,
    in_output_time: *const AudioTimeStamp,
    in_client_data: *mut c_void,
) -> OSStatus;

const fn fourcc(s: &[u8; 4]) -> u32 {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
}

const kAudioTapPropertyFormat: u32 = fourcc(b"tfmt");
const kAudioObjectPropertyScopeGlobal: u32 = fourcc(b"glob");
const kAudioObjectPropertyElementMain: u32 = 0;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioHardwareCreateProcessTap(
        in_description: *mut AnyObject,
        out_tap_id: *mut AudioObjectID,
    ) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(tap_id: AudioObjectID) -> OSStatus;
    fn AudioHardwareCreateAggregateDevice(
        in_description: CFDictionaryRef,
        out_device_id: *mut AudioObjectID,
    ) -> OSStatus;
    fn AudioHardwareDestroyAggregateDevice(device_id: AudioObjectID) -> OSStatus;
    fn AudioObjectGetPropertyData(
        in_object: AudioObjectID,
        in_address: *const AudioObjectPropertyAddress,
        in_qualifier_data_size: u32,
        in_qualifier_data: *const c_void,
        io_data_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;
    fn AudioDeviceCreateIOProcID(
        in_device: AudioObjectID,
        in_proc: AudioDeviceIOProc,
        in_client_data: *mut c_void,
        out_proc_id: *mut AudioDeviceIOProcID,
    ) -> OSStatus;
    fn AudioDeviceDestroyIOProcID(device: AudioObjectID, proc_id: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStart(device: AudioObjectID, proc_id: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStop(device: AudioObjectID, proc_id: AudioDeviceIOProcID) -> OSStatus;
}

const RING_CAPACITY_SAMPLES: usize = super::aec::SAMPLE_RATE as usize * 4;
const PREROLL_MS: u64 = 100;

pub struct SystemTap {
    ring: Arc<Mutex<VecDeque<f32>>>,
    state_ptr: *mut CallbackState,
    proc_id: AudioDeviceIOProcID,
    aggregate_id: AudioObjectID,
    tap_id: AudioObjectID,
}

unsafe impl Send for SystemTap {}
unsafe impl Sync for SystemTap {}

struct CallbackState {
    ring: Arc<Mutex<VecDeque<f32>>>,
    src_channels: u32,
    step: f64,
    next_read: f64,
    input_count: u64,
    prev_in: f32,
}

unsafe impl Send for CallbackState {}

impl SystemTap {
    pub fn start() -> Result<Self, String> {
        unsafe {
            let cls = class!(CATapDescription);
            let alloc_obj: *mut AnyObject = msg_send![cls, alloc];
            if alloc_obj.is_null() {
                return Err("CATapDescription alloc に失敗しました。".to_string());
            }
            let empty_array: *mut AnyObject = msg_send![class!(NSArray), array];
            let description: *mut AnyObject =
                msg_send![alloc_obj, initStereoMixdownOfProcesses: empty_array];
            if description.is_null() {
                return Err("CATapDescription init に失敗しました。".to_string());
            }

            let nsuuid: *mut AnyObject = msg_send![class!(NSUUID), UUID];
            let _: () = msg_send![description, setUUID: nsuuid];
            let _: () = msg_send![description, setMuteBehavior: 0i32];

            let mut tap_id: AudioObjectID = 0;
            let status = AudioHardwareCreateProcessTap(description, &mut tap_id);

            let uuid_str: String = {
                let uuid_string_obj: *mut AnyObject = msg_send![nsuuid, UUIDString];
                let utf8_ptr: *const c_char = msg_send![uuid_string_obj, UTF8String];
                std::ffi::CStr::from_ptr(utf8_ptr)
                    .to_string_lossy()
                    .into_owned()
            };

            let _: () = msg_send![description, release];

            if status != 0 || tap_id == 0 {
                return Err(format!(
                    "AudioHardwareCreateProcessTap が失敗しました (OSStatus={status})。macOS 14.4 以上が必要です。"
                ));
            }

            let mut format = AudioStreamBasicDescription {
                sample_rate: 48000.0,
                format_id: 0,
                format_flags: 0,
                bytes_per_packet: 0,
                frames_per_packet: 0,
                bytes_per_frame: 0,
                channels_per_frame: 2,
                bits_per_channel: 32,
                reserved: 0,
            };
            let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
            let addr = AudioObjectPropertyAddress {
                selector: kAudioTapPropertyFormat,
                scope: kAudioObjectPropertyScopeGlobal,
                element: kAudioObjectPropertyElementMain,
            };
            let _ = AudioObjectGetPropertyData(
                tap_id,
                &addr,
                0,
                ptr::null(),
                &mut size,
                &mut format as *mut _ as *mut c_void,
            );

            let aggregate_uid = format!("Enja-Tap-{uuid_str}");
            let agg_dict = build_aggregate_description(&aggregate_uid, &uuid_str);

            let mut aggregate_id: AudioObjectID = 0;
            let status = AudioHardwareCreateAggregateDevice(
                agg_dict.as_concrete_TypeRef(),
                &mut aggregate_id,
            );
            if status != 0 || aggregate_id == 0 {
                AudioHardwareDestroyProcessTap(tap_id);
                return Err(format!(
                    "AudioHardwareCreateAggregateDevice が失敗しました (OSStatus={status})。"
                ));
            }

            let ring = Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY_SAMPLES)));
            let channels = if format.channels_per_frame > 0 {
                format.channels_per_frame
            } else {
                2
            };
            let sample_rate = if format.sample_rate > 0.0 {
                format.sample_rate
            } else {
                48000.0
            };
            let state = Box::new(CallbackState {
                ring: ring.clone(),
                src_channels: channels,
                step: sample_rate / super::aec::SAMPLE_RATE as f64,
                next_read: 0.0,
                input_count: 0,
                prev_in: 0.0,
            });
            let state_ptr = Box::into_raw(state);

            let mut proc_id: AudioDeviceIOProcID = ptr::null_mut();
            let status = AudioDeviceCreateIOProcID(
                aggregate_id,
                io_proc,
                state_ptr as *mut c_void,
                &mut proc_id,
            );
            if status != 0 || proc_id.is_null() {
                drop(Box::from_raw(state_ptr));
                AudioHardwareDestroyAggregateDevice(aggregate_id);
                AudioHardwareDestroyProcessTap(tap_id);
                return Err(format!(
                    "AudioDeviceCreateIOProcID が失敗しました (OSStatus={status})。"
                ));
            }

            let status = AudioDeviceStart(aggregate_id, proc_id);
            if status != 0 {
                AudioDeviceDestroyIOProcID(aggregate_id, proc_id);
                drop(Box::from_raw(state_ptr));
                AudioHardwareDestroyAggregateDevice(aggregate_id);
                AudioHardwareDestroyProcessTap(tap_id);
                return Err(format!(
                    "AudioDeviceStart が失敗しました (OSStatus={status})。"
                ));
            }

            std::thread::sleep(std::time::Duration::from_millis(PREROLL_MS));

            Ok(Self {
                ring,
                state_ptr,
                proc_id,
                aggregate_id,
                tap_id,
            })
        }
    }

    pub fn take_reference(&self, frame_samples: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; frame_samples];
        if let Ok(mut guard) = self.ring.lock() {
            let take = frame_samples.min(guard.len());
            for slot in out.iter_mut().take(take) {
                *slot = guard.pop_front().unwrap_or(0.0);
            }
        }
        out
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        unsafe {
            if !self.proc_id.is_null() && self.aggregate_id != 0 {
                AudioDeviceStop(self.aggregate_id, self.proc_id);
                AudioDeviceDestroyIOProcID(self.aggregate_id, self.proc_id);
            }
            if self.aggregate_id != 0 {
                AudioHardwareDestroyAggregateDevice(self.aggregate_id);
            }
            if self.tap_id != 0 {
                AudioHardwareDestroyProcessTap(self.tap_id);
            }
            if !self.state_ptr.is_null() {
                drop(Box::from_raw(self.state_ptr));
            }
        }
    }
}

fn build_aggregate_description(
    aggregate_uid: &str,
    tap_uuid_str: &str,
) -> CFDictionary<CFType, CFType> {
    let mut tap_entry = CFMutableDictionary::<CFType, CFType>::new();
    tap_entry.add(
        &CFString::new("uid").as_CFType(),
        &CFString::new(tap_uuid_str).as_CFType(),
    );
    tap_entry.add(
        &CFString::new("drift").as_CFType(),
        &CFBoolean::true_value().as_CFType(),
    );

    let taps_array = CFArray::from_CFTypes(&[tap_entry.to_immutable().as_CFType()]);

    let mut dict = CFMutableDictionary::<CFType, CFType>::new();
    dict.add(
        &CFString::new("name").as_CFType(),
        &CFString::new("Enja Tap").as_CFType(),
    );
    dict.add(
        &CFString::new("uid").as_CFType(),
        &CFString::new(aggregate_uid).as_CFType(),
    );
    dict.add(
        &CFString::new("private").as_CFType(),
        &CFNumber::from(1i32).as_CFType(),
    );
    dict.add(
        &CFString::new("stacked").as_CFType(),
        &CFNumber::from(0i32).as_CFType(),
    );
    dict.add(
        &CFString::new("tapautostart").as_CFType(),
        &CFNumber::from(1i32).as_CFType(),
    );
    dict.add(&CFString::new("taps").as_CFType(), &taps_array.as_CFType());
    dict.to_immutable()
}

unsafe extern "C" fn io_proc(
    _in_device: AudioObjectID,
    _in_now: *const AudioTimeStamp,
    in_input_data: *const AudioBufferList,
    _in_input_time: *const AudioTimeStamp,
    _out_output_data: *mut AudioBufferList,
    _in_output_time: *const AudioTimeStamp,
    in_client_data: *mut c_void,
) -> OSStatus {
    if in_input_data.is_null() || in_client_data.is_null() {
        return 0;
    }
    let state = &mut *(in_client_data as *mut CallbackState);
    let list = &*in_input_data;
    if list.number_buffers == 0 {
        return 0;
    }
    let first = &list.buffers[0];
    if first.data.is_null() || first.data_byte_size == 0 {
        return 0;
    }

    let n_samples = first.data_byte_size as usize / std::mem::size_of::<f32>();
    let samples = std::slice::from_raw_parts(first.data as *const f32, n_samples);
    let channels = state.src_channels.max(1) as usize;
    let frames = n_samples / channels;

    let est_capacity = ((frames as f64 / state.step).ceil() as usize) + 2;
    let mut emitted: Vec<f32> = Vec::with_capacity(est_capacity);

    for f in 0..frames {
        let mut sum = 0.0_f32;
        for c in 0..channels {
            sum += samples[f * channels + c];
        }
        let mono = (sum / channels as f32).clamp(-1.0, 1.0);

        let cur = state.input_count as f64;
        let prev = cur - 1.0;
        while state.next_read <= cur {
            let frac = (state.next_read - prev) as f32;
            let value = state.prev_in + (mono - state.prev_in) * frac;
            emitted.push(value.clamp(-1.0, 1.0));
            state.next_read += state.step;
        }
        state.prev_in = mono;
        state.input_count += 1;
    }

    if !emitted.is_empty() {
        if let Ok(mut guard) = state.ring.lock() {
            for v in emitted {
                if guard.len() >= RING_CAPACITY_SAMPLES {
                    guard.pop_front();
                }
                guard.push_back(v);
            }
        }
    }
    0
}
