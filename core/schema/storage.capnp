@0xea97e42fee03194b;

interface Sealed(T) {}

interface Sealer {
    seal @0 [T] (x :T) -> (s :Sealed(T));
}

interface Unsealer {
    unseal @0 [T] (s :Sealed(T)) -> (x :T);
}

interface BulkSave {
    save @0 (data :AnyPointer) -> (s :Data);
    restore @1 (s :Data) -> (data :AnyPointer);
}

interface Encrypter {
    encrypt @0 (data :Data) -> (code :Data);
}

interface Decrypter {
    decrypt @0 (code :Data) -> (data :Data);
}

interface Secrecy {
    mkSealer @0 () -> (seal :Sealer, unseal :Unsealer);
    mkEncryptor @1 () -> (encrypt :Encrypter, decrypt :Decrypter);
}


interface SturdyRef(T) {
    # A capability representing a storable reference to something; modules shouldn't implement this themselves and instead rely on keystone to provide and manage them
    # Keystone will take responsibility for creating and transferring secure handles to this which can persist through disconnections; from the perspective of a module, these will always appear to be a live capability

    restore @0 () -> (cap :T);
}

interface Saveable(T) {
    # An interface to implement on an object that wants to be able to be stored; to implement these use the keystone provided Save interface and provide data your module understands how to restore
    save @0 () -> (ref :SturdyRef(T));
}

interface Save(Storage) {
    # Keystone provides this, specialized to whatever type your module requests its storage to be in. To implement Saveable, create a whatever storage you want, call save from this interface on keystone's capability, and return the result.
    save @0 [T] (data :Storage) -> (ref :SturdyRef(T));
}

interface Restore(Storage) {
    # Implement this on your module's root object and take the exact same data as you provide to save, returning what it restores to.
    restore @0 [T] (data :Storage) -> (cap :T);
}

struct ConnectionInfo {
    # Internal implementation detail; only use if working on keystone itself or an external service that talks to keystone servers without using keystone.
    # How to find a keystone instance that you've connected with before.

}

struct RestorationToken {
    token @0 :AnyPointer;
}

struct RawSturdyRef {
    # Internal implementation detail; only use if working on keystone itself or an external service that talks to keystone servers without using keystone.
    # What to store in order to be able to recover a capability reference offline.
    connectioninfo @0 :ConnectionInfo;
    restorationtoken @1 :RestorationToken;
}

interface SaveRaw {
    # Internal implementation detail; only use if working on keystone itself or an external service that talks to keystone servers without using keystone.
    # When you ask keystone to save a remote reference, it will internally use this interface between servers; the userspace on both sides will see the Save interface instead.
    save @0 () -> (ref :RawSturdyRef);
}

interface RestoreRaw {
    # Internal implementation detail; only use if working on keystone itself or an external service that talks to keystone servers without using keystone.
    # Keystone's bootstrap interface exposed to external machines will implement this, and keystone will internally use this interface between servers; the userspace on both sides will see the Restore interface instead.

    restore @0 (s :RestorationToken) -> (c :AnyPointer);
}

interface Cell(T) {
    get @0 () -> (data :T);
    set @1 (data :T);
}

interface ObjectStore {
    interface Object(T) {
        get @0 () -> (x :T);
    }
    store @0 [T] (x :T) -> (obj :Object(T));
}