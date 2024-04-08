@0xdfd607b5c71b738a;

# A Bootable module takes some configuration type T and returns some root interface C. This is not the same 
# as a SturdyRef save or restore, which is captured in a seperate interface.
interface BootModule(T, C) {
  bootstrap @0 (parameters :T) -> (root_capability :C);
}