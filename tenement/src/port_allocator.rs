//! Port allocator for auto-assigning TCP ports to instances
//!
//! Manages a pool of ports in the range 30000-40000.
//! Automatically assigns free ports to instances and tracks allocations.

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Port range for auto-allocation
const PORT_MIN: u16 = 30000;
const PORT_MAX: u16 = 40000;

/// Port allocator that manages a pool of TCP ports
///
/// Ports are allocated from the range 30000-40000 on a first-available basis.
/// Released ports are returned to the pool and can be reused.
///
/// Thread-safe: uses RwLock for concurrent access.
#[derive(Debug)]
pub struct PortAllocator {
    /// Set of currently allocated ports
    allocated: Arc<RwLock<HashSet<u16>>>,
    /// Next port to try allocating (optimization to avoid scanning from start)
    next_port: Arc<RwLock<u16>>,
}

impl PortAllocator {
    /// Create a new port allocator
    pub fn new() -> Self {
        Self {
            allocated: Arc::new(RwLock::new(HashSet::new())),
            next_port: Arc::new(RwLock::new(PORT_MIN)),
        }
    }

    /// Allocate a free port from the pool
    ///
    /// Returns the allocated port number, or an error if no ports are available.
    ///
    /// # Example
    /// ```
    /// # use tenement::PortAllocator;
    /// # tokio_test::block_on(async {
    /// let allocator = PortAllocator::new();
    /// let port = allocator.allocate().await.unwrap();
    /// assert!(port >= 30000 && port <= 40000);
    /// # })
    /// ```
    pub async fn allocate(&self) -> anyhow::Result<u16> {
        let mut allocated = self.allocated.write().await;
        let mut next_port = self.next_port.write().await;

        // Try to find a free port starting from next_port
        let start_port = *next_port;
        let mut current_port = start_port;

        loop {
            if !allocated.contains(&current_port) {
                // Found a free port
                allocated.insert(current_port);
                *next_port = if current_port == PORT_MAX {
                    PORT_MIN
                } else {
                    current_port + 1
                };
                return Ok(current_port);
            }

            // Move to next port, wrapping around
            current_port = if current_port == PORT_MAX {
                PORT_MIN
            } else {
                current_port + 1
            };

            // If we've wrapped around to the start, no ports available
            if current_port == start_port {
                anyhow::bail!(
                    "No free ports available in range {}-{}. {} ports allocated.",
                    PORT_MIN,
                    PORT_MAX,
                    allocated.len()
                );
            }
        }
    }

    /// Release a port back to the pool
    ///
    /// The port becomes available for future allocations.
    /// Safe to call even if the port wasn't allocated (no-op).
    ///
    /// # Example
    /// ```
    /// # use tenement::PortAllocator;
    /// # tokio_test::block_on(async {
    /// let allocator = PortAllocator::new();
    /// let port = allocator.allocate().await.unwrap();
    /// allocator.release(port).await;
    /// // Port can now be allocated again
    /// let port2 = allocator.allocate().await.unwrap();
    /// assert_eq!(port, port2);
    /// # })
    /// ```
    pub async fn release(&self, port: u16) {
        let mut allocated = self.allocated.write().await;
        allocated.remove(&port);
    }

    /// Get the number of currently allocated ports
    pub async fn allocated_count(&self) -> usize {
        let allocated = self.allocated.read().await;
        allocated.len()
    }

    /// Get the number of available ports
    pub async fn available_count(&self) -> usize {
        let total = (PORT_MAX - PORT_MIN + 1) as usize;
        total - self.allocated_count().await
    }

    /// Check if a specific port is currently allocated
    pub async fn is_allocated(&self, port: u16) -> bool {
        let allocated = self.allocated.read().await;
        allocated.contains(&port)
    }
}

impl Default for PortAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_allocate_single_port() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate().await.unwrap();
        assert!(port >= PORT_MIN && port <= PORT_MAX);
    }

    #[tokio::test]
    async fn test_allocate_multiple_ports() {
        let allocator = PortAllocator::new();
        let port1 = allocator.allocate().await.unwrap();
        let port2 = allocator.allocate().await.unwrap();
        let port3 = allocator.allocate().await.unwrap();

        // All should be in range and different
        assert!(port1 >= PORT_MIN && port1 <= PORT_MAX);
        assert!(port2 >= PORT_MIN && port2 <= PORT_MAX);
        assert!(port3 >= PORT_MIN && port3 <= PORT_MAX);
        assert_ne!(port1, port2);
        assert_ne!(port2, port3);
        assert_ne!(port1, port3);
    }

    #[tokio::test]
    async fn test_release_port() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate().await.unwrap();

        assert_eq!(allocator.allocated_count().await, 1);
        assert!(allocator.is_allocated(port).await);

        allocator.release(port).await;

        assert_eq!(allocator.allocated_count().await, 0);
        assert!(!allocator.is_allocated(port).await);
    }

    #[tokio::test]
    async fn test_release_and_reallocate() {
        let allocator = PortAllocator::new();
        let port1 = allocator.allocate().await.unwrap();

        // Port1 should be allocated
        assert!(allocator.is_allocated(port1).await);

        // Release it
        allocator.release(port1).await;

        // Port should now be available again
        assert!(!allocator.is_allocated(port1).await);

        // Allocate another port - this will be the next one (port1 + 1)
        let port2 = allocator.allocate().await.unwrap();
        assert_ne!(port1, port2);

        // Allocate all remaining ports except port1
        let total = (PORT_MAX - PORT_MIN + 1) as usize;
        for _ in 0..(total - 2) {  // -2 because we've allocated port2 and want to leave port1 free
            allocator.allocate().await.unwrap();
        }

        // Now allocate again - should get port1 back (it wraps around to find it)
        let port3 = allocator.allocate().await.unwrap();
        assert_eq!(port1, port3);
    }

    #[tokio::test]
    async fn test_allocated_count() {
        let allocator = PortAllocator::new();
        assert_eq!(allocator.allocated_count().await, 0);

        allocator.allocate().await.unwrap();
        assert_eq!(allocator.allocated_count().await, 1);

        allocator.allocate().await.unwrap();
        assert_eq!(allocator.allocated_count().await, 2);

        allocator.allocate().await.unwrap();
        assert_eq!(allocator.allocated_count().await, 3);
    }

    #[tokio::test]
    async fn test_available_count() {
        let allocator = PortAllocator::new();
        let total = (PORT_MAX - PORT_MIN + 1) as usize;

        assert_eq!(allocator.available_count().await, total);

        allocator.allocate().await.unwrap();
        assert_eq!(allocator.available_count().await, total - 1);

        allocator.allocate().await.unwrap();
        assert_eq!(allocator.available_count().await, total - 2);
    }

    #[tokio::test]
    async fn test_is_allocated() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate().await.unwrap();

        assert!(allocator.is_allocated(port).await);
        assert!(!allocator.is_allocated(PORT_MIN + 1000).await); // Random port
    }

    #[tokio::test]
    async fn test_release_unallocated_port_is_safe() {
        let allocator = PortAllocator::new();
        // Releasing a port that wasn't allocated should be safe (no-op)
        allocator.release(PORT_MIN + 500).await;
        assert_eq!(allocator.allocated_count().await, 0);
    }

    #[tokio::test]
    async fn test_wrap_around() {
        let allocator = PortAllocator::new();

        // Allocate first 3 ports
        let port1 = allocator.allocate().await.unwrap();
        let port2 = allocator.allocate().await.unwrap();
        let port3 = allocator.allocate().await.unwrap();

        // Verify they're sequential
        assert_eq!(port1, PORT_MIN);
        assert_eq!(port2, PORT_MIN + 1);
        assert_eq!(port3, PORT_MIN + 2);

        // Release the first two ports
        allocator.release(port1).await;
        allocator.release(port2).await;

        // Now we have port3 allocated, and port1 and port2 free
        // Allocate all remaining ports (filling up the pool except for port1 and port2)
        let total = (PORT_MAX - PORT_MIN + 1) as usize;
        // We have 1 allocated (port3), 2 free (port1, port2), and (total - 3) remaining
        for _ in 0..(total - 3) {
            allocator.allocate().await.unwrap();
        }

        // Now only port1 and port2 are free
        assert_eq!(allocator.allocated_count().await, total - 2);

        // Allocate next - should wrap around and find port1
        let port_wrapped1 = allocator.allocate().await.unwrap();
        assert_eq!(port_wrapped1, port1);

        // Allocate one more - should find port2
        let port_wrapped2 = allocator.allocate().await.unwrap();
        assert_eq!(port_wrapped2, port2);
    }

    #[tokio::test]
    async fn test_allocate_all_ports() {
        let allocator = PortAllocator::new();
        let total = (PORT_MAX - PORT_MIN + 1) as usize;

        let mut ports = Vec::new();
        for _ in 0..total {
            ports.push(allocator.allocate().await.unwrap());
        }

        // All ports should be allocated
        assert_eq!(allocator.allocated_count().await, total);
        assert_eq!(allocator.available_count().await, 0);

        // Trying to allocate another should fail
        let result = allocator.allocate().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No free ports"));
    }

    #[tokio::test]
    async fn test_concurrent_allocations() {
        let allocator = Arc::new(PortAllocator::new());

        let mut handles = vec![];
        for _ in 0..100 {
            let alloc = allocator.clone();
            handles.push(tokio::spawn(async move {
                alloc.allocate().await.unwrap()
            }));
        }

        let mut ports = HashSet::new();
        for handle in handles {
            let port = handle.await.unwrap();
            ports.insert(port);
        }

        // All ports should be unique
        assert_eq!(ports.len(), 100);
        assert_eq!(allocator.allocated_count().await, 100);
    }

    #[tokio::test]
    async fn test_concurrent_allocate_and_release() {
        let allocator = Arc::new(PortAllocator::new());

        let mut handles = vec![];

        // Spawn tasks that allocate and release
        for i in 0..50 {
            let alloc = allocator.clone();
            handles.push(tokio::spawn(async move {
                let port = alloc.allocate().await.unwrap();
                if i % 2 == 0 {
                    alloc.release(port).await;
                }
                port
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // About half should be released (those with even i)
        let count = allocator.allocated_count().await;
        assert!(count >= 20 && count <= 30); // Allow some variance
    }

    #[tokio::test]
    async fn test_port_range_boundaries() {
        let allocator = PortAllocator::new();
        let port = allocator.allocate().await.unwrap();

        // First port should be at the minimum
        assert_eq!(port, PORT_MIN);

        // Allocate all remaining ports
        let total = (PORT_MAX - PORT_MIN) as usize;
        for _ in 0..total {
            allocator.allocate().await.unwrap();
        }

        // Last port should be at the maximum
        let allocated = allocator.allocated.read().await;
        assert!(allocated.contains(&PORT_MAX));
    }
}
