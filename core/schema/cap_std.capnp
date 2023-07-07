@0xd5b73ad4256c863f


interface File {
  sync_all @0 () -> ();
  sync_data @1 () -> ();
  set_let @2 (size :UInt64) -> ();
  metadata @3 () -> (metadata :Metadata);
  try_clone @4 () -> (cloned :File);
  set_permissions @5 (perm :Permissions) -> ();
  open_ambient @6 (path :Text, ambient_authority :AmbientAuthority) -> (file :File);
  create_ambient @7 (path :Text, ambient_authority :AmbientAuthority) -> (file :File);
  open_ambient_with @8 (path :Text, options :OpenOptions, ambient_authority :AmbientAuthority) -> (file :File);
  options @9 () -> (options :OpenOptions);
}

inteface Dir {
  open @0 (path :Text) -> (file :File);
  open_with @1 (path :Text, options :OpenOptions) -> (file :File);
  open_dir @2 (path :Text) -> (dir :Dir);
  create_dir @3 (path :Text) -> ();
  create_dir_all @4 (path :Text) -> ();
  create_dir_with @5 (path :Text, dir_builder :DirBuilder) -> ();
  create @6 (path :Text) -> (file :File);
  canonicalize @7 (path :Text) -> (path_buf :Text);
  copy @8 (path_from :Text, path_to :Text) -> (result: UInt64);
  hard_link @9 (src_path :Text, dst_path :Text) -> ();
  metadata @10 (path: Text) -> (metadata :Metadata);
  dir_metadata @11 () -> (metadata :Metadata);
  entries @12 () -> (iter :ReadDir);
  read_dir @13 (path :Text) -> (iter :ReadDir);
  read @14 (path :Text) -> (result :List(UInt8));
  read_link @15 (path :Text) -> (result :Text);
  read_to_string @16 (path :Text) -> (result :Text);
  remove_dir @17 (path :Text) -> ();
  remove_dir_all @18 (path :Text) -> ();
  remove_open_dir @19 () -> ();
  remove_open_dir_all @20 () -> ();
  remove_file @21 (path :Text) -> ();
  rename @22 (from :Text, to :Text) -> ();
  set_permissions @23 (path :Text, perm :Permissions) -> ();
  symlink_metadata @24 (path :Text) -> (metadata :Metadata);
  write @25 (path :Text, contents :List(UInt8)) -> ();
  symlink @26 (original :Text, link :Text) -> ();

  #Several unimplemented unix functions in cap std
  
  try_clone @27 () -> (directory :Directory);
  exists @28 (path :Text) -> (result :Bool);
  try_exists @29 (path :Text) -> (result :Bool);
  is_file @30 (path :Text) -> (result :Bool);
  is_dir @31 (path :Text) -> (result :Bool);
  open_ambient_dir @32 (path :Text, ambient_authority :AmbientAuthority) -> (result :Dir);
  open_parent_dir @33 (ambient_authority :AmbientAuthority) -> (result :Dir);
  create_ambient_dir_all @34 (path :Text, ambient_authority :AmbientAuthority) -> ();
  reopen_dir @35 (dir :Filelike) -> (result :Dir); #Filelike = primitive type trait thing
}

interface Permissions {
  readonly @0 () -> (result :Bool);
  set_readonly @1 (readonly :Bool) -> ();
}

interface Metadata {
  file_type @0 () -> (file_type :FileType);
  is_dir @1 () -> (result :Bool);
  is_file @2 () -> (result :Bool);
  is_symlink @3 () -> (result :Bool);
  len @4 () -> (result :UInt64);
  permissions @5 () -> (permissions :Permissions);
  modified @6 () -> (time :SystemTime);
  accessed @7 () -> (time :SystemTime);
  created @8 () -> (time :SystemTime);
}

struct Duration {
    secs @0 :UInt64;
    nanos @1 :UInt32
}

interface Instant {
  duration_since @0 (earlier :Instant) -> (duration :Duration);
  checked_duration_since @1 (earlier :Instant) -> (duration :Duration);
  saturating_duration_since @2 (earlier :Instant) -> (duration :Duration);
  checked_add @3 (duration :Duration) -> (instant :Instant);
  checked_sub @4 (duration :Duration) -> (instant :Instant);
}

interface MonotonicClock {
  new @0 (ambient_authority :AmbientAuthority) -> (clock :MonotonicClock);
  now @1 () -> (instant :Instant);
  elapsed @2 (instant :Instant) -> (duration :Duration);
}

interface SystemClock {
  new @0 (ambient_authority :AmbientAuthority) -> (clock :SystemClock);
  now @1 () -> (time :SystemTime);
  struct Elapsed_Result {
    union {
      duration @0 :Duration;
      error @1 :SystemTimeError;
    }
  }
  elapsed @2 (system_time :SystemTime) -> (result :Elapsed_Result);
}

interface SystemTime {
  duration_since @0 (earlier :SystemTime) -> (duration :Duration);
  checked_add @1 (duration :Duration) -> (result :SystemTime);
  checked_sub @2 (duration :Duration) -> (result :SystemTime);
}

interface SystemTimeError {
  duration @0 () -> (duration :Duration);
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
  next @0 () -> (iter :ReadDir);
  get @1 () -> (entry :DirEntry);
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

interface ProjectDirs {
  from @0 (qualifier :Text, organization :Text, application :Text, ambient_authority :AmbientAuthority) -> (project_dirs :ProjectDirs);
  cache_dir @1 () -> (dir :Dir);
  config_dir @2 () -> (dir :Dir);
  data_dir @3 () -> (dir :Dir);
  data_local_dir @4 () -> (dir :Dir);
  runtime_dir @5 () -> (dir :Dir);
}

interface UserDirs {
  new @0 () -> (user_dirs :UserDirs);
  home_dir @1 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  audio_dir @2 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  desktop_dir @3 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  document_dir @4 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  download_dir @5 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  font_dir @6 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  picture_dir @7 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  public_dir @8 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  template_dir @9 (ambient_authority :AmbientAuthority) -> (dir :Dir);
  video_dir @10 (ambient_authority :AmbientAuthority) -> (dir :Dir);
}

interface TempDir extends(Dir) {
  new @0 (ambient_authority :AmbientAuthority) -> (temp_dir :TempDir);
  new_in @1 (dir :Dir) -> (temp_dir :TempDir);
  close @2 () -> ();
}

interface TempFile {
  new @0 (dir :Dir) -> (temp_file :TempFile);
  new_anonymous @1 (dir :Dir) -> (file :File);
  as_file @2 () -> (file :File);
  as_file_mut @3 () -> (file :File);
  replace @4 (dest :Text) -> ();
}