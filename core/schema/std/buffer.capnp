@0xecc1a71b6e9b626b;

interface ReadableMemoryBuffer {
    read @0 (offset :UInt64, size :UInt64) -> (result :Data);
    # If possible, instances of this type should be sent with a file descriptor suitable for mmap to avoid unnecessary copies.
}