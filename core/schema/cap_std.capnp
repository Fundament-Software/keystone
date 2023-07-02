#id 

struct fs_file {
  union {
    #Maybe required for sending files over and communicating with other code
  }
}

struct Std_File {
  inner @0 :fs_file;
}

struct Cap_Std_File {
  std @0 :Std_File;
}

interface File {
  from_std @1 (std :Std_File) -> (file :File);
  into_std @2 () -> (std :Std_File);
  sync_all @3 () -> ();
  sync_data @4 () -> ();
  set_let @5 (size :UInt64) -> ();
  metadata @6 () -> (metadata :Metadata);
  try_clone @7 () -> (cloned :Cap_Std_File);
  set_permissions @8 (perm :Permissions) -> ();
  open_ambient @9 (path :Text, ambient_authority :AmbientAuthority) -> (file :File);
  create_ambient @10 (path :Text, ambient_authority :AmbientAuthority) -> (file :File);
  open_ambient_with @11 (path :Text, options :OpenOptions, ambient_authority :AmbientAuthority) -> (file :File);
  options @12 () -> (options :OpenOptions);
}

struct Directory {
  std_file @0 :Std_File;
}

inteface Dir {
  from_std_file @0 (std_file :Std_File) -> (file :File);
  into_std_file @1 () -> (std_file :Std_File);
  open @2 (path :Text) -> (file :File);
  open_with @3 (path :Text, options :OpenOptions) -> (file :File);
  open_dir @4 (path :Text) -> (dir :Dir);
  create_dir @5 (path :Text) -> ();
  create_dir_all @6 (path :Text) -> ();
  create_dir_with @7 (path :Text, dir_builder :DirBuilder) -> ();
  create @8 (path :Text) -> (file :File);
  canonicalize @9 (path :Text) -> (path_buf :Text);
  copy @10 (path_from :Text, path_to :Text) -> (result: UInt64);
  hard_link @11 (src_path :Text, dst_path :Text) -> ();
  metadata @12 (path: Text) -> (metadata :Metadata);
  dir_metadata @13 () -> (metadata :Metadata);
  entries @14 () -> (iter :ReadDir);
  read_dir @15 (path :Text) -> (iter :ReadDir);
  read @16 (path :Text) -> (result :List(UInt8));
  read_link @17 (path :Text) -> (result :Text);
  read_to_string @18 (path :Text) -> (result :Text);
  remove_dir @19 (path :Text) -> ();
  remove_dir_all @20 (path :Text) -> ();
  remove_open_dir @21 () -> ();
  remove_open_dir_all @22 () -> ();
  remove_file @23 (path :Text) -> ();
  rename @24 (from :Text, to :Text) -> ();
  set_permissions @25 (path :Text, perm :Permissions) -> ();
  symlink_metadata @26 (path :Text) -> (metadata :Metadata);
  write @27 (path :Text, contents :List(UInt8)) -> ();
  symlink @28 (original :Text, link :Text) -> ();

  #Several unimplemented unix functions in cap std
  
  try_clone @29 () -> (directory :Directory);
  exists @30 (path :Text) -> (result :Bool);
  try_exists @31 (path :Text) -> (result :Bool);
  is_file @32 (path :Text) -> (result :Bool);
  is_dir @33 (path :Text) -> (result :Bool);
  open_ambient_dir @34 (path :Text, ambient_authority :AmbientAuthority) -> (result :Dir);
  open_parent_dir @35 (ambient_authority :AmbientAuthority) -> (result :Dir);
  create_ambient_dir_all @36 (path :Text, ambient_authority :AmbientAuthority) -> ();
  reopen_dir @37 (dir :Filelike) -> (result :Dir); #Filelike = primitive type trait thing
}

struct Std_Permissions {
  #.....
}

interface Permissions {
  from_std @0 (std :Std_Permissions) -> (permissions :Permissions);
  into_std @1 (file :Std_File) -> (std :Std_Permissions);
  readonly @2 () -> (result :Bool);
  set_readonly @3 (readonly :Bool) -> ();
}

interface Metadata {
  from_file @0 (file :Std_File) -> (metadata :Metadata);
  from_just_metadata @1 (std :Std_Metadata) -> (metadata :Metadata);
  file_type @2 () -> (file_type :FileType);
  is_dir @3 () -> (result :Bool);
  is_file @4 () -> (result :Bool);
  is_symlink @5 () -> (result :Bool);
  len @6 () -> (result :UInt64);
  permissions @7 () -> (permissions :Permissions);
  modified @8 () -> (time :SystemTime);
  accessed @9 () -> (time :SystemTime);
  created @10 () -> (time :SystemTime);
}
struct Std_System_Time {
  #....
}
struct Duration {
    secs @0 :UInt64;
    nanos @1 :UInt32
}
interface SystemTime {
  from_std @0 (std :Std_System_Time) -> (result :SystemTime);
  into_std @1 () -> (std :Std_System_Time);
  duration_since @2 (earlier :SystemTime) -> (duration :Duration);
  checked_add @3 (duration :Duration) -> (result :SystemTime);
  checked_sub @4 (duration :Duration) -> (result :SystemTime);
}

interface DirBuilder {
  new @0 () -> (builder :DirBuilder);
  recursive @1 (recursive :Bool) -> (builder :DirBuilder);
  options @2 () -> (options :DirOptions);
  is_recursive @3 () -> (result :Bool);
}

interface FileType {
  dir @0 () -> (file_type :FileType);
  file @1 () -> (file_type :FileType);
  unknown @2 () -> (file_type :FileType);
  is_dir @3 () -> (result :Bool);
  is_file @4 () -> (result :Bool);
  is_symlink @5 () -> (result :Bool);
}

interface ReadDir {
  next @0 () -> (entry :DirEntry);
}

interface DirEntry {
  open @0 () -> (file :File);
  open_with @1 (options :OpenOptions) -> (file :File);
  open_dir @2 () -> (dir :Dir);
  remove_file @3 () -> ();
  remove_dir @4 () -> ();
  metadata @5 () -> (metadata :Metadata);
  file_type @6 () -> (type :FileType);
  file_name @7 () -> (result :Text);
}

struct AmbientAuthority {

}