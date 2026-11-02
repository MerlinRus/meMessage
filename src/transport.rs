use std::{io, mem::size_of, ptr};
use windows_sys::Win32::{Devices::Bluetooth::*, Networking::WinSock::*};

fn socket_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

pub fn find_iphone() -> io::Result<u64> {
    let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: 1,
        fReturnRemembered: 1,
        fReturnConnected: 1,
        ..Default::default()
    };
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    // No inquiry, new pairing, or permission changes: only enumerate existing bonds.
    let handle = unsafe { BluetoothFindFirstDevice(&search, &mut info) };
    if handle.is_null() {
        return Err(io::Error::other(
            "Сопряжённый iPhone не найден. Подключи его в настройках Bluetooth Windows.",
        ));
    }
    let mut named = Vec::new();
    let mut phones = Vec::new();
    loop {
        let length = info
            .szName
            .iter()
            .position(|&x| x == 0)
            .unwrap_or(info.szName.len());
        let name = String::from_utf16_lossy(&info.szName[..length]);
        let address = unsafe { info.Address.Anonymous.ullLong };
        if name.to_lowercase().contains("iphone") {
            named.push(address);
        }
        if (info.ulClassofDevice >> 8) & 0x1f == 2 {
            phones.push(address);
        }
        if unsafe { BluetoothFindNextDevice(handle, &mut info) } == 0 {
            break;
        }
    }
    unsafe {
        BluetoothFindDeviceClose(handle);
    }
    let found = if named.is_empty() { phones } else { named };
    if found.len() != 1 {
        return Err(io::Error::other(
            "Нужен один сопряжённый iPhone. Отключи лишние телефоны и повтори подключение.",
        ));
    }
    Ok(found[0])
}

pub struct Transport {
    socket: SOCKET,
}
impl Transport {
    pub fn connect(address: u64) -> io::Result<Self> {
        Self::connect_service(address, 0x1132)
    }
    pub fn connect_service(address: u64, service: u16) -> io::Result<Self> {
        let mut data = WSADATA::default();
        let error = unsafe { WSAStartup(0x0202, &mut data) };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
        let socket = unsafe { WSASocketW(32, SOCK_STREAM, 3, ptr::null(), 0, 0) };
        if socket == INVALID_SOCKET {
            unsafe {
                WSACleanup();
            }
            return Err(socket_error());
        }
        let client = Self { socket };
        client.option(3, SO_BTH_AUTHENTICATE as i32, 1)?;
        client.option(3, SO_BTH_ENCRYPT as i32, 1)?;
        client.option(SOL_SOCKET, SO_RCVTIMEO, 15_000)?;
        client.option(SOL_SOCKET, SO_SNDTIMEO, 15_000)?;
        let mut endpoint = [0u8; 30];
        endpoint[..2].copy_from_slice(&32u16.to_le_bytes());
        endpoint[2..10].copy_from_slice(&address.to_le_bytes());
        endpoint[10..26].copy_from_slice(&[
            0x32, 0x11, 0, 0, 0, 0, 0, 0x10, 0x80, 0, 0, 0x80, 0x5f, 0x9b, 0x34, 0xfb,
        ]);
        endpoint[10..12].copy_from_slice(&service.to_le_bytes());
        let mut nonblocking = 1u32;
        if unsafe { ioctlsocket(socket, FIONBIO, &mut nonblocking) } != 0 {
            return Err(socket_error());
        }
        let result = unsafe { connect(socket, endpoint.as_ptr().cast(), endpoint.len() as i32) };
        if result != 0 {
            let error = unsafe { WSAGetLastError() };
            if error != WSAEWOULDBLOCK {
                return Err(io::Error::from_raw_os_error(error));
            }
            let mut write = FD_SET {
                fd_count: 1,
                ..Default::default()
            };
            write.fd_array[0] = socket;
            let mut except = write;
            let timeout = TIMEVAL {
                tv_sec: 12,
                tv_usec: 0,
            };
            let ready = unsafe { select(0, ptr::null_mut(), &mut write, &mut except, &timeout) };
            if ready == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "iPhone не ответил. Проверь Bluetooth и оставь телефон рядом.",
                ));
            }
            if ready < 0 {
                return Err(socket_error());
            }
            let mut error = 0i32;
            let mut length = 4;
            if unsafe {
                getsockopt(
                    socket,
                    SOL_SOCKET,
                    SO_ERROR,
                    (&mut error as *mut i32).cast(),
                    &mut length,
                )
            } != 0
            {
                return Err(socket_error());
            }
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
        }
        nonblocking = 0;
        if unsafe { ioctlsocket(socket, FIONBIO, &mut nonblocking) } != 0 {
            return Err(socket_error());
        }
        Ok(client)
    }
    fn option(&self, level: i32, option: i32, value: i32) -> io::Result<()> {
        if unsafe { setsockopt(self.socket, level, option, (&value as *const i32).cast(), 4) }
            == SOCKET_ERROR
        {
            return Err(socket_error());
        }
        Ok(())
    }
    fn read_exact(&mut self, mut buffer: &mut [u8]) -> io::Result<()> {
        while !buffer.is_empty() {
            let count = unsafe { recv(self.socket, buffer.as_mut_ptr(), buffer.len() as i32, 0) };
            if count < 0 {
                return Err(socket_error());
            }
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "iPhone закрыл соединение.",
                ));
            }
            buffer = &mut buffer[count as usize..];
        }
        Ok(())
    }
}

pub trait Exchange {
    fn exchange(&mut self, packet: &[u8]) -> io::Result<Vec<u8>>;
}
impl Exchange for Transport {
    fn exchange(&mut self, mut packet: &[u8]) -> io::Result<Vec<u8>> {
        while !packet.is_empty() {
            let count = unsafe { send(self.socket, packet.as_ptr(), packet.len() as i32, 0) };
            if count < 0 {
                return Err(socket_error());
            }
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Соединение прервано.",
                ));
            }
            packet = &packet[count as usize..];
        }
        let mut header = [0u8; 3];
        self.read_exact(&mut header)?;
        let length = u16::from_be_bytes([header[1], header[2]]) as usize;
        if !(3..=4096).contains(&length) {
            return Err(io::Error::other("Некорректная длина ответа iPhone."));
        }
        let mut result = vec![0u8; length];
        result[..3].copy_from_slice(&header);
        self.read_exact(&mut result[3..])?;
        Ok(result)
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.socket);
            WSACleanup();
        }
    }
}
