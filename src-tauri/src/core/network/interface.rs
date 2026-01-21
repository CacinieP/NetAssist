/// Network interface information
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    pub name: String,
    pub display_name: String,
    pub ipv4_addresses: Vec<String>,
    pub ipv6_addresses: Vec<String>,
    pub is_up: bool,
    pub is_loopback: bool,
}

/// Network interface manager
pub struct InterfaceManager;

impl InterfaceManager {
    pub fn new() -> Self {
        Self
    }

    /// Get all network interfaces
    pub fn get_interfaces(&self) -> anyhow::Result<Vec<NetworkInterface>> {
        let platform_interfaces = crate::platform::get_network_interfaces()?;

        let interfaces = platform_interfaces.into_iter().map(|info| {
            NetworkInterface {
                name: info.name,
                display_name: info.display_name,
                ipv4_addresses: info.ipv4_addresses.into_iter().map(|ip| ip.to_string()).collect(),
                ipv6_addresses: info.ipv6_addresses.into_iter().map(|ip| ip.to_string()).collect(),
                is_up: info.is_up,
                is_loopback: info.is_loopback,
            }
        }).collect();

        Ok(interfaces)
    }

    /// Get the primary interface (default route)
    pub fn get_primary_interface(&self) -> anyhow::Result<Option<NetworkInterface>> {
        let interfaces = self.get_interfaces()?;

        // Return the first non-loopback interface that's up
        for interface in interfaces {
            if interface.is_up && !interface.is_loopback && !interface.ipv4_addresses.is_empty() {
                return Ok(Some(interface));
            }
        }

        Ok(None)
    }

    /// Get interface by name
    pub fn get_interface_by_name(&self, name: &str) -> anyhow::Result<Option<NetworkInterface>> {
        let interfaces = self.get_interfaces()?;

        Ok(interfaces.into_iter().find(|i| i.name == name))
    }
}

impl Default for InterfaceManager {
    fn default() -> Self {
        Self::new()
    }
}
