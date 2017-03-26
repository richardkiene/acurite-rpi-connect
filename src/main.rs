extern crate libusb;

use std::slice;
use std::time::Duration;
use std::thread;

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8
}

static LIBUSB_REQUEST_TYPE_CLASS: u8 = (0x01 << 5);
static LIBUSB_RECIPIENT_INTERFACE: u8 = 0x01;
static LIBUSB_ENDPOINT_IN: u8 = 0x80;
static REPORT_ONE: u16 = 0x01;
static REPORT_TWO: u16 = 0x02;
static READ_REQUEST: u8 = 0x01;
static READ_VALUE: u16 = 0x0100;
static READ_INDEX: u16 = 0x00;

fn main() {
    let vid: u16 = 9408;
    let pid: u16 = 3;

    match libusb::Context::new() {
        Ok(mut context) => {
            match open_device(&mut context, vid, pid) {
                Some((mut device, device_desc, mut handle)) => read_device(&mut device, &device_desc, &mut handle).unwrap(),
                None => println!("could not find device {:04x}:{:04x}", vid, pid)
            }
        },
        Err(e) => panic!("could not initialize libusb: {}", e)
    }
}

fn open_device(context: &mut libusb::Context, vid: u16, pid: u16) -> Option<(libusb::Device, libusb::DeviceDescriptor, libusb::DeviceHandle)> {
    let devices = match context.devices() {
        Ok(d) => d,
        Err(_) => return None
    };

    for device in devices.iter() {
        let device_desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue
        };

        if device_desc.vendor_id() == vid && device_desc.product_id() == pid {
            match device.open() {
                Ok(handle) => return Some((device, device_desc, handle)),
                Err(_) => continue
            }
        }
    }

    None
}

fn read_device(device: &mut libusb::Device, device_desc: &libusb::DeviceDescriptor, handle: &mut libusb::DeviceHandle) -> libusb::Result<()> {
    try!(handle.reset());

    let timeout = Duration::from_secs(1);
    let languages = try!(handle.read_languages(timeout));

    println!("Active configuration: {}", try!(handle.active_configuration()));
    println!("Languages: {:?}", languages);

    if languages.len() > 0 {
        let language = languages[0];

        println!("Manufacturer: {:?}", handle.read_manufacturer_string(language, device_desc, timeout).ok());
        println!("Product: {:?}", handle.read_product_string(language, device_desc, timeout).ok());
        println!("Serial Number: {:?}", handle.read_serial_number_string(language, device_desc, timeout).ok());
    }

    match find_readable_endpoint(device, device_desc) {
        Some(endpoint) => read_endpoint(handle, endpoint),
        None => println!("No readable control endpoint")
    }

    Ok(())
}

fn find_readable_endpoint(device: &mut libusb::Device, device_desc: &libusb::DeviceDescriptor) -> Option<Endpoint> {
    for n in 0..device_desc.num_configurations() {
        let config_desc = match device.config_descriptor(n) {
            Ok(c) => c,
            Err(_) => continue
        };

        for interface in config_desc.interfaces() {
            for interface_desc in interface.descriptors() {
                for endpoint_desc in interface_desc.endpoint_descriptors() {
                    if endpoint_desc.direction() == libusb::Direction::In {
                        return Some(Endpoint {
                            config: config_desc.number(),
                            iface: interface_desc.interface_number(),
                            setting: interface_desc.setting_number(),
                            address: endpoint_desc.address()
                        });
                    }
                }
            }
        }
    }

    None
}

fn read_endpoint(handle: &mut libusb::DeviceHandle, endpoint: Endpoint) {
    println!("Reading from endpoint: {:?}", endpoint);

    let has_kernel_driver = match handle.kernel_driver_active(endpoint.iface) {
        Ok(true) => {
            handle.detach_kernel_driver(endpoint.iface).ok();
            true
        },
        _ => false
    };

    println!(" - kernel driver? {}", has_kernel_driver);

    match configure_endpoint(handle, &endpoint) {
        Ok(_) => {
            let mut vec = Vec::<u8>::with_capacity(256);
            let mut buf = unsafe { slice::from_raw_parts_mut((&mut vec[..]).as_mut_ptr(), vec.capacity()) };

            let timeout = Duration::from_secs(30);
            let mut counter: u64 = 0;
            loop {
                thread::sleep(Duration::from_millis(1000));
                
                /* Fetch REPORT_ONE */
                if counter % 10 == 0 {
                    match handle.read_control(
                        LIBUSB_REQUEST_TYPE_CLASS | LIBUSB_RECIPIENT_INTERFACE | LIBUSB_ENDPOINT_IN,
                        0x01,
                        0x0100 + REPORT_ONE,
                        0,
                        buf,
                        timeout) {
                        Ok(len) => {
                            unsafe { vec.set_len(len) };

                            if (vec[3] & 0x0f) == 1 {
                                let wind_speed: f32 = (((vec[4] & 0x1f) << 3) | ((vec[5] & 0x70) >> 7)) as f32 * 0.62;
                                let wind_dir: u8 = vec[5] & 0x0f;
                                let rain_count: u8 = vec[7] & 0x7f;

                                println!("wind speed: {:?} wind dir: {:?} rain count: {:?}", wind_speed, wind_dir, rain_count);
                            }

                            if (vec[3] & 0x0f) == 8 {
                                let wind_speed: f32 = (((vec[4] & 0x1f) << 3) | ((vec[5] & 0x70) >> 7)) as f32 * 0.62;
                                let temp: f32 = ((((vec[5] & 0x0f) >> 7) | (vec[6] & 0x7f)) as f32 - 400.00) / 10.0;
                                let humidity: u8 = vec[7] & 0x7f;

                                println!("wind speed: {:?} temp: {:?} humidity: {:?}", wind_speed, temp, humidity);
                            }
                        },
                        Err(err) => println!("could not read from endpoint: {}", err)
                    }
                }

                /* Fetch REPORT_TWO */
                if counter % 30 == 0 {
                    match handle.read_control(
                        LIBUSB_REQUEST_TYPE_CLASS | LIBUSB_RECIPIENT_INTERFACE | LIBUSB_ENDPOINT_IN,
                        READ_REQUEST,
                        READ_VALUE + REPORT_TWO,
                        READ_INDEX,
                        buf,
                        timeout) {
                        Ok(len) => {
                            unsafe { vec.set_len(len) };
                            /*println!(" - read: {:?}", vec); */
                        },
                        Err(err) => println!("could not read from endpoint: {}", err)
                    }
                }

                /* Show latest data
                if counter % 15 == 0 {
                    println!("TODO: Output buffer here");
                } */

                counter = counter + 1;
            }
        },
        Err(err) => println!("could not configure endpoint: {}", err)
    }

    if has_kernel_driver {
        handle.attach_kernel_driver(endpoint.iface).ok();
    }
}

fn configure_endpoint<'a>(handle: &'a mut libusb::DeviceHandle, endpoint: &Endpoint) -> libusb::Result<()> {
    try!(handle.set_active_configuration(endpoint.config));
    try!(handle.claim_interface(endpoint.iface));
    try!(handle.set_alternate_setting(endpoint.iface, endpoint.setting));
    Ok(())
}
