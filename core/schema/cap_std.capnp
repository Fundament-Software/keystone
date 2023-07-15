@0xd5b73ad4256c863f

interface CapFs {
  use_ambient_authority @0 () -> (ambient_authority :AmbientAuthority);
  dir_open @1 (path :Text) -> (dir :Dir);
  dir_builder_new @2 () -> (builder :DirBuilder);
  temp_dir_new_in @3 (dir :Dir) -> (temp_dir :TempDir);
  temp_file_new @4 (dir :Dir) -> (temp_file :TempFile);
  temp_file_new_anonymous @5 (dir :Dir) -> (file :File);
}

using Stream = import "byte_stream.capnp".ByteStream;

interface File {
  sync_all @0 () -> ();
  sync_data @1 () -> ();
  set_let @2 (size :UInt64) -> ();
  metadata @3 () -> (metadata :Metadata);
  try_clone @4 () -> (cloned :File);
  set_permissions @5 (perm :Permissions) -> ();
  options @6 () -> (options :OpenOptions);
  open @7 () -> (stream :Stream);
}

inteface Dir {
  open @0 (path :Text) -> (file :File);
  open_with @1 (path :Text, options :OpenOptions) -> (file :File);
  create_dir @2 (path :Text) -> ();
  create_dir_all @3 (path :Text) -> ();
  create_dir_with @4 (path :Text, dir_builder :DirBuilder) -> ();
  create @5 (path :Text) -> (file :File);
  canonicalize @6 (path :Text) -> (path_buf :Text);
  copy @7 (path_from :Text, path_to :Text) -> (result: UInt64);
  hard_link @8 (src_path :Text, dst_path :Text) -> ();
  metadata @9 (path: Text) -> (metadata :Metadata);
  dir_metadata @10 () -> (metadata :Metadata);
  entries @11 () -> (iter :ReadDir);
  read_dir @12 (path :Text) -> (iter :ReadDir);
  read @13 (path :Text) -> (result :List(UInt8));
  read_link @14 (path :Text) -> (result :Text);
  read_to_string @15 (path :Text) -> (result :Text);
  remove_dir @16 (path :Text) -> ();
  remove_dir_all @17 (path :Text) -> ();
  remove_open_dir @18 () -> ();
  remove_open_dir_all @19 () -> ();
  remove_file @20 (path :Text) -> ();
  rename @21 (from :Text, to :Text) -> ();
  set_permissions @22 (path :Text, perm :Permissions) -> ();
  symlink_metadata @23 (path :Text) -> (metadata :Metadata);
  write @24 (path :Text, contents :List(UInt8)) -> ();
  symlink @25 (original :Text, link :Text) -> ();

  #Several unimplemented unix functions in cap std
  
  try_clone @26 () -> (directory :Directory);
  exists @27 (path :Text) -> (result :Bool);
  try_exists @28 (path :Text) -> (result :Bool);
  is_file @29 (path :Text) -> (result :Bool);
  is_dir @30 (path :Text) -> (result :Bool);
  reopen_dir @31 (dir :Filelike) -> (result :Dir); #Filelike = primitive type trait thing
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
  now @0 () -> (instant :Instant);
  elapsed @1 (instant :Instant) -> (duration :Duration);
}

interface SystemClock {
  now @0 () -> (time :SystemTime);
  struct Elapsed_Result {
    union {
      duration @0 :Duration;
      error @1 :SystemTimeError;
    }
  }
  elapsed @1 (system_time :SystemTime) -> (result :Elapsed_Result);
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
  recursive @0 (recursive :Bool) -> (builder :DirBuilder);
  options @1 () -> (options :DirOptions);
  is_recursive @2 () -> (result :Bool);
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

interface AmbientAuthority {
  file_open_ambient @0 (path :Text) -> (file :File);
  file_create_ambient @1 (path :Text) -> (file :File);
  file_open_ambient_with @2 (path :Text, options :OpenOptions) -> (file :File);
  dir_open_ambient @3 (path :Text) -> (result :Dir);
  dir_open_parent @4 () -> (result :Dir);
  dir_create_ambient_all @5 (path :Text) -> ();
  monotonic_clock_new @6 () -> (clock :MonotonicClock);
  system_clock_new @7 () -> (clock :SystemClock);
  project_dirs_from @8 (qualifier :Text, organization :Text, application :Text) -> (project_dirs :ProjectDirs);
  user_dirs_home_dir @9 () -> (dir :Dir);
  user_dirs_audio_dir @10 () -> (dir :Dir);
  user_dirs_desktop_dir @11 () -> (dir :Dir);
  user_dirs_document_dir @12 () -> (dir :Dir);
  user_dirs_download_dir @13 () -> (dir :Dir);
  user_dirs_font_dir @14 () -> (dir :Dir);
  user_dirs_picture_dir @15 () -> (dir :Dir);
  user_dirs_public_dir @16 () -> (dir :Dir);
  user_dirs_template_dir @17 () -> (dir :Dir);
  user_dirs_video_dir @18 () -> (dir :Dir);
  temp_dir_new @19 () -> (temp_dir :TempDir);
}

interface ProjectDirs {
  cache_dir @0 () -> (dir :Dir);
  config_dir @1 () -> (dir :Dir);
  data_dir @2 () -> (dir :Dir);
  data_local_dir @3 () -> (dir :Dir);
  runtime_dir @4 () -> (dir :Dir);
}

interface UserDirs {
  new @0 () -> (user_dirs :UserDirs);
}

interface TempDir extends(Dir) {
  close @0 () -> ();
}

interface TempFile {
  as_file @0 () -> (file :File);
  as_file_mut @1 () -> (file :File);
  replace @2 (dest :Text) -> ();
}