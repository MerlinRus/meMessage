use windows::{
    Devices::Bluetooth::{BluetoothDevice, Rfcomm::RfcommServiceId},
    Storage::Streams::DataReader,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
    core::GUID,
};

fn unsigned(bytes: &[u8]) -> Option<u32> {
    match bytes {
        [0x08, value] => Some(*value as u32),
        [0x09, a, b] => Some(u16::from_be_bytes([*a, *b]) as u32),
        [0x0a, a, b, c, d] => Some(u32::from_be_bytes([*a, *b, *c, *d])),
        _ => None,
    }
}
pub fn probe() -> Result<serde_json::Value, String> {
    let address = crate::transport::find_iphone().map_err(|error| error.to_string())?;
    let result = (|| -> windows::core::Result<serde_json::Value> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let device = BluetoothDevice::FromBluetoothAddressAsync(address)?.get()?;
        let service_id =
            RfcommServiceId::FromUuid(GUID::from_u128(0x00001132_0000_1000_8000_00805f9b34fb))?;
        let services = device
            .GetRfcommServicesForIdAsync(&service_id)?
            .get()?
            .Services()?;
        let mut reports = Vec::new();
        for service in services {
            let attributes = service.GetSdpRawAttributesAsync()?.get()?;
            let read = |id| -> Option<u32> {
                let buffer = attributes.Lookup(id).ok()?;
                let reader = DataReader::FromBuffer(&buffer).ok()?;
                let len = reader.UnconsumedBufferLength().ok()? as usize;
                if len > 5 {
                    return None;
                }
                let mut bytes = vec![0; len];
                reader.ReadBytes(&mut bytes).ok()?;
                unsigned(&bytes)
            };
            reports.push(serde_json::json!({"supported_message_types_raw":read(0x0316),"map_supported_features_raw":read(0x0317)}));
        }
        Ok(serde_json::json!({"services":reports}))
    })();
    result.map_err(|error| error.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdp_unsigned_values_are_big_endian_and_reject_other_types() {
        assert_eq!(unsigned(&[0x08, 2]), Some(2));
        assert_eq!(unsigned(&[0x09, 1, 2]), Some(258));
        assert_eq!(unsigned(&[0x0a, 0, 0, 1, 2]), Some(258));
        assert_eq!(unsigned(&[0x08, 1, 2]), None);
        assert_eq!(unsigned(&[0x28, 1]), None);
    }
}
