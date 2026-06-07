use crate::interpret::{InterpretError, values::Pointer};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Byte {
    Init(u8, Option<usize>),
    Uninit,
}
impl Byte {
    pub fn prov(self) -> Option<usize> {
        match self {
            Self::Init(_, prov) => prov,
            _ => None,
        }
    }
    pub fn data(self) -> Option<u8> {
        match self {
            Self::Init(value, _) => Some(value),
            Self::Uninit => None,
        }
    }
    pub fn from_u8(value: u8) -> Self {
        Self::Init(value, None)
    }
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MemLocation {
    Heap,
    Stack,
    Function,
}
#[derive(Debug)]
pub struct Allocation {
    pub live: bool,
    pub base_address: usize,
    pub location: MemLocation,
    pub bytes: Vec<Byte>,
}
#[derive(Debug)]
pub struct Memory {
    allocations: Vec<Allocation>,
    next_address: usize,
}
impl Memory {
    pub fn new() -> Self {
        Self {
            allocations: Vec::new(),
            next_address: 1000,
        }
    }
    pub fn leaked_allocations(self) -> Vec<Allocation> {
        self.allocations
            .into_iter()
            .filter(|alloc| alloc.location == MemLocation::Heap && alloc.live)
            .collect()
    }
    pub fn allocate(&mut self, location: MemLocation, size: usize) -> Pointer {
        let alloc_id = self.allocations.len();
        let addr = self.next_address.next_multiple_of(size.max(1));
        let allocation = Allocation {
            live: true,
            base_address: addr,
            location,
            bytes: vec![Byte::Uninit; size],
        };
        self.next_address = addr + size + 1;
        self.allocations.push(allocation);
        Pointer {
            address: addr,
            alloc: Some(alloc_id),
        }
    }
    pub fn deallocate(
        &mut self,
        location: MemLocation,
        ptr: Pointer,
    ) -> Result<(), InterpretError> {
        let Some(alloc) = ptr.alloc else {
            return Err(InterpretError::FreeInvalid);
        };
        let allocation = &mut self.allocations[alloc];
        if allocation.base_address != ptr.address {
            return Err(InterpretError::BaseMismatch);
        }
        if allocation.location != location {
            return Err(InterpretError::DeallocMismatch {
                expected: allocation.location,
                got: location,
            });
        }
        if !allocation.live {
            return Err(InterpretError::DoubleFree);
        }
        allocation.live = false;
        Ok(())
    }
    fn check_valid_for_access(
        &self,
        pointer: Pointer,
        size: usize,
    ) -> Result<Option<(usize, usize)>, InterpretError> {
        if size == 0 {
            return Ok(None);
        }
        let Some(alloc) = pointer.alloc else {
            return Err(InterpretError::InvalidPointer);
        };
        let allocation = &self.allocations[alloc];
        if !allocation.live {
            return Err(InterpretError::UsedDeallocatedMemory);
        }
        let offset = (pointer.address as isize) - (allocation.base_address as isize);
        if offset < 0 || (offset as usize) + size > allocation.bytes.len() {
            println!("Out of bounds big dawg {}", size);
            return Err(InterpretError::OutOfBounds {
                len: allocation.bytes.len(),
                offset,
            });
        }
        Ok(Some((alloc, offset as usize)))
    }
    pub fn write(&mut self, pointer: Pointer, bytes: Vec<Byte>) -> Result<(), InterpretError> {
        let Some((alloc, offset)) = self.check_valid_for_access(pointer, bytes.len())? else {
            return Ok(());
        };
        self.allocations[alloc].bytes[offset..][..bytes.len()].copy_from_slice(&bytes);
        Ok(())
    }
    pub fn read(&self, pointer: Pointer, size: usize) -> Result<Vec<Byte>, InterpretError> {
        let Some((alloc, offset)) = self.check_valid_for_access(pointer, size)? else {
            return Ok(Vec::new());
        };
        let bytes = self.allocations[alloc].bytes[offset..][..size].to_vec();
        Ok(bytes)
    }
    pub fn byte_offset(
        &self,
        pointer: Pointer,
        offset: isize,
    ) -> Result<Pointer, (Pointer, isize, usize)> {
        let new_address = usize::try_from(pointer.address as isize + offset).unwrap();
        let Some(alloc) = pointer.alloc else {
            return Ok(Pointer {
                address: new_address,
                alloc: None,
            });
        };
        let allocation = &self.allocations[alloc];
        let offset = (new_address as isize) - (allocation.base_address as isize);
        if offset < 0 || offset > allocation.bytes.len() as isize {
            return Err((
                Pointer {
                    address: new_address,
                    alloc: pointer.alloc,
                },
                offset,
                allocation.bytes.len(),
            ));
        }
        Ok(Pointer {
            address: new_address,
            alloc: pointer.alloc,
        })
    }
    pub fn byte_offset_in_bounds(
        &self,
        pointer: Pointer,
        offset: isize,
    ) -> Result<Pointer, InterpretError> {
        let pointer = match self.byte_offset(pointer, offset) {
            Ok(pointer) => pointer,
            Err((_, offset, len)) => {
                return Err(InterpretError::OutOfBounds { len, offset });
            }
        };
        Ok(pointer)
    }
}
