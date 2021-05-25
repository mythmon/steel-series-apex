use std::{convert::TryFrom, fmt, marker::PhantomData, time::Duration};

use anyhow::{anyhow, ensure, Context, Result};
use bitvec::prelude::*;
use bitvec::{order, prelude::BitVec, slice::BitSlice};
use rusb::{Device, UsbContext};
use tracing::warn;

pub struct KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    dev: Device<C>,
    _keyboard_type: PhantomData<K>,
}

impl<K, C> KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    pub fn new(context: &C) -> Result<Self> {
        let dev = context
            .devices()
            .context("Getting list of devices")?
            .iter()
            .map(|dev| {
                let desc = dev
                    .device_descriptor()
                    .context("getting device description")?;
                if desc.product_id() == K::PRODUCT_ID && desc.vendor_id() == K::VENDOR_ID {
                    Ok(Some(dev))
                } else {
                    Ok(None)
                }
            })
            .filter_map(|dev_res: Result<_>| match dev_res {
                Ok(opt) => opt,
                Err(error) => {
                    warn!(%error, "Error reading USB device");
                    None
                }
            })
            .next()
            .ok_or_else(|| anyhow!("Could not find keyboard"))?;

        Ok(Self {
            dev,
            _keyboard_type: PhantomData::default(),
        })
    }

    fn send(&self, cmd: KeyboardCommand, buf: &[u8]) -> Result<()> {
        let mut handle = self.dev.open().context("Opening USB device for keyboard")?;
        const INTERFACE_NUM: u8 = 1;
        handle
            .set_auto_detach_kernel_driver(true)
            .context("settings auto-detach kernel driver")?;

        let dev_desc = self.dev.device_descriptor()?;
        for config_num in 0..(dev_desc.num_configurations()) {
            let config_desc = self.dev.config_descriptor(config_num)?;
            for iface_num in 0..(config_desc.num_interfaces()) {
                handle.claim_interface(iface_num).context(format!(
                    "claiming config {}/{}, interface {}/{}",
                    config_num,
                    dev_desc.num_configurations(),
                    iface_num,
                    config_desc.num_interfaces(),
                ))?;
            }
        }

        let request_type = rusb::request_type(
            rusb::Direction::Out,
            rusb::RequestType::Class,
            rusb::Recipient::Interface,
        );
        assert_eq!(request_type, 0x21);
        let request = 0x09; // what does this mean?
        let mut remaining_bytes = buf.len();

        let timeout = Duration::from_secs(5);

        let bytes_written = handle
            .write_control(
                request_type,
                request,
                cmd.value(),
                cmd.index(),
                buf,
                timeout,
            )
            .context(format!("sending {:?} request", cmd))?;
        remaining_bytes = remaining_bytes.saturating_sub(bytes_written);
        ensure!(remaining_bytes == 0, "entire request not written");

        Ok(())
    }

    fn send_image(&self, image_data: &BitSlice<Msb0, u8>) -> Result<()> {
        let mut io_buf: BitVec<order::Msb0, u8> = BitVec::new();
        io_buf.resize(8, false);
        io_buf[..8].store(0x65_u8);
        io_buf.extend_from_bitslice(image_data);

        let buf: &[u8] = io_buf.as_raw_slice();
        self.send(KeyboardCommand::Oled, buf)
    }

    pub fn checkerboard(&self, x_size: usize, y_size: usize) -> Result<()> {
        let mut image_data: BitVec<Msb0, u8> = BitVec::new();
        image_data.resize(K::OLED_SIZE.area(), false);

        let x_size2 = x_size * 2;
        let y_size2 = y_size * 2;

        for y in 0..K::OLED_SIZE.height {
            for x in 0..K::OLED_SIZE.width {
                let p = x + y * K::OLED_SIZE.width;
                let mut bit = image_data
                    .get_mut(p)
                    .ok_or_else(|| anyhow!("Index out of bounds"))?;
                *bit = (x % x_size2 < x_size) ^ (y % y_size2 < y_size)
            }
        }

        self.send_image(&image_data)
    }

    pub fn blank(&self) -> Result<()> {
        let mut image_data: BitVec<Msb0, u8> = BitVec::new();
        image_data.resize(K::OLED_SIZE.area(), false);
        self.send_image(&image_data)
    }
}

impl<K, C> fmt::Debug for KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        K::fmt_debug(f)
    }
}

#[derive(Debug, Clone, Copy)]
enum KeyboardCommand {
    #[allow(dead_code)]
    Colors,
    #[allow(dead_code)]
    Config {
        index: u16,
    },
    Oled,
}

impl KeyboardCommand {
    fn value(&self) -> u16 {
        match self {
            KeyboardCommand::Colors => 0x300,
            KeyboardCommand::Config { .. } => 0x200,
            KeyboardCommand::Oled => 0x300,
        }
    }

    fn index(&self) -> u16 {
        match self {
            KeyboardCommand::Colors => 0x01,
            KeyboardCommand::Config { index } => *index,
            KeyboardCommand::Oled => 0x01,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Size {
    width: usize,
    height: usize,
}

impl Size {
    fn area(&self) -> usize {
        self.width * self.height
    }
}

pub trait KeyboardType {
    const VENDOR_ID: u16;
    const PRODUCT_ID: u16;
    const OLED_SIZE: Size;

    fn fmt_debug(f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x?}:{:x?}", Self::VENDOR_ID, Self::PRODUCT_ID)
    }
}

#[derive(Clone, Copy)]
pub struct ApexProTkl;

impl KeyboardType for ApexProTkl {
    const VENDOR_ID: u16 = 0x1038;
    const PRODUCT_ID: u16 = 0x1614;
    const OLED_SIZE: Size = Size {
        width: 128,
        height: 40,
    };
}

impl<K: KeyboardType, C: UsbContext> TryFrom<Device<C>> for KeyboardDevice<K, C> {
    type Error = anyhow::Error;

    fn try_from(dev: Device<C>) -> Result<Self, Self::Error> {
        let desc = dev.device_descriptor()?;
        anyhow::ensure!(
            desc.vendor_id() == K::VENDOR_ID && desc.product_id() == K::PRODUCT_ID,
            "unexpected device"
        );

        Ok(Self {
            dev,
            _keyboard_type: PhantomData::default(),
        })
    }
}
