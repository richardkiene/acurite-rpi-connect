extern crate libusb;

use std::slice;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8
}

static LIBUSB_REQUEST_TYPE_CLASS: u8  = (0x01 << 5);
static LIBUSB_RECIPIENT_INTERFACE: u8 = 0x01;
static LIBUSB_ENDPOINT_IN: u8 = 0x80;

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
            match handle.read_control(
                LIBUSB_REQUEST_TYPE_CLASS | LIBUSB_RECIPIENT_INTERFACE | LIBUSB_ENDPOINT_IN,
                0x01,
                0x0100+1,
                0,
                buf,
                timeout) {
                Ok(len) => {
                    unsafe { vec.set_len(len) };
                    println!(" - read: {:?}", vec);
                },
                Err(err) => println!("could not read from endpoint: {}", err)
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
