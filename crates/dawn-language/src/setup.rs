use std::net::IpAddr;

pub struct Setup {
    pub controllers: Vec<Controller>,
    pub patch: Patch,
    pub layout: Layout,
}

pub struct Controller {
    pub key: String,
    pub address: Option<ControllerAddress>,
    pub protocol: Protocol,
    pub outputs: Vec<ControllerOutput>,
}

pub enum Protocol {
    E131,
}

pub struct ControllerAddress {
    pub ip: IpAddr,
    pub port: u16,
}

pub struct ControllerOutput {
    pub channel_order: RgbChannelOrder,
    pub pixels: usize,
    pub first_universe: u32,
}

pub enum RgbChannelOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

pub struct Patch {}

pub struct Layout {}
