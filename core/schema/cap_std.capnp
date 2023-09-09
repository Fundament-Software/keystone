@0xd5b73ad4256c863f;

interface CapFs {
  useAmbientAuthority @0 () -> (ambientAuthority :AmbientAuthority);
  dirOpen @1 (path :Text) -> (dir :Dir);
  #tempDirNewIn @2 (dir :Dir) -> (tempDir :TempDir);
  #tempFileNew @3 (dir :Dir) -> (tempFile :TempFile);
  #tempFileNewAnonymous @4 (dir :Dir) -> (file :File);
}

using Stream = import "std/byte_stream.capnp".ByteStream;

interface File {
  syncAll @0 () -> ();
  syncData @1 () -> ();
  setLen @2 (size :UInt64) -> ();
  metadata @3 () -> (metadata :Metadata);
  tryClone @4 () -> (cloned :File);
  setReadonly @5 (readonly :Bool) -> ();
  open @6 () -> (stream :Stream);
}

interface Dir {
  open @0 (path :Text) -> (file :File);
  openWith @1 (path :Text, openOptions :OpenOptions) -> (file :File);
  createDir @2 (path :Text) -> ();
  createDirAll @3 (path :Text) -> ();
  create @4 (path :Text) -> (file :File);
  canonicalize @5 (path :Text) -> (pathBuf :Text);
  copy @6 (pathFrom :Text, pathTo :Text) -> (result :UInt64);
  hardLink @7 (srcPath :Text, dstPath :Text) -> ();
  metadata @8 (path :Text) -> (metadata :Metadata);
  dirMetadata @9 () -> (metadata :Metadata);
  entries @10 () -> (iter :ReadDir);
  readDir @11 (path :Text) -> (iter :ReadDir);
  read @12 (path :Text) -> (result :Data);
  readLink @13 (path :Text) -> (result :Text);
  readToString @14 (path :Text) -> (result :Text);
  removeDir @15 (path :Text) -> ();
  removeDirAll @16 (path :Text) -> ();
  removeOpenDir @17 () -> ();
  removeOpenDirAll @18 () -> ();
  removeFile @19 (path :Text) -> ();
  rename @20 (from :Text, to :Text) -> ();
  setReadonly @21 (path :Text, readonly :Bool) -> ();
  symlinkMetadata @22 (path :Text) -> (metadata :Metadata);
  write @23 (path :Text, contents :Data) -> ();
  symlink @24 (original :Text, link :Text) -> ();

  #Several unimplemented unix functions in cap std
  
  exists @25 (path :Text) -> (result :Bool);
  tryExists @26 (path :Text) -> (result :Bool);
  isFile @27 (path :Text) -> (result :Bool);
  isDir @28 (path :Text) -> (result :Bool);
  tempDirNewIn @29 () -> (tempDir :TempDir);
  tempFileNew @30 () -> (tempFile :TempFile);
  tempFileNewAnonymous @31 () -> (file :File);
}

interface Permissions {
  readonly @0 () -> (result :Bool);
  setReadonly @1 (readonly :Bool) -> ();
}

interface Metadata {
  fileType @0 () -> (fileType :FileType);
  isDir @1 () -> (result :Bool);
  isFile @2 () -> (result :Bool);
  isSymlink @3 () -> (result :Bool);
  len @4 () -> (result :UInt64);
  permissions @5 () -> (permissions :Permissions);
  modified @6 () -> (time :SystemTime);
  accessed @7 () -> (time :SystemTime);
  created @8 () -> (time :SystemTime);
}

struct Duration {
  secs @0 :UInt64;
  nanos @1 :UInt32;
}

interface Instant {
  durationSince @0 (earlierDurationSinceUnixEpoch :Duration) -> (duration :Duration);
  checkedDurationSince @1 (earlierDurationSinceUnixEpoch :Duration) -> (duration :Duration);
  saturatingDurationSince @2 (earlierDurationSinceUnixEpoch :Duration) -> (duration :Duration);
  checkedAdd @3 (duration :Duration) -> (instant :Instant);
  checkedSub @4 (duration :Duration) -> (instant :Instant);
  getDurationSinceUnixEpoch @5 () -> (duration :Duration);
}

interface MonotonicClock {
  now @0 () -> (instant :Instant);
  elapsed @1 (instant :Instant) -> (duration :Duration);
}

interface SystemClock {
  now @0 () -> (time :SystemTime);
  elapsed @1 (durationSinceUnixEpoch :Duration) -> (result :Duration);
}

interface SystemTime {
  durationSince @0 (durationSinceUnixEpoch :Duration) -> (duration :Duration);
  checkedAdd @1 (duration :Duration) -> (result :SystemTime);
  checkedSub @2 (duration :Duration) -> (result :SystemTime);
  getDurationSinceUnixEpoch @3 () -> (duration :Duration);
}

interface SystemTimeError {
  duration @0 () -> (duration :Duration);
}

interface FileType {
  dir @0 () -> (fileType :FileType);
  file @1 () -> (fileType :FileType);
  unknown @2 () -> (fileType :FileType);
  isDir @3 () -> (result :Bool);
  isFile @4 () -> (result :Bool);
  isSymlink @5 () -> (result :Bool);
}

interface ReadDir {
  next @0 () -> (entry :DirEntry);
}

struct OpenOptions {
  read @0 :Bool;
  write @1 :Bool;
  append @2 :Bool;
  truncate @3 :Bool;
  create @4 :Bool;
  createNew @5 :Bool;
}

#interface OpenOptions {
#  read @0 (read :Bool) -> (openOptions :OpenOptions);
#  write @1 (write :Bool) -> (openOptions :OpenOptions);
#  append @2 (append :Bool) -> (openOptions :OpenOptions);
#  truncate @3 (truncate :Bool) -> (openOptions :OpenOptions);
#  create @4 (create :Bool) -> (openOptions :OpenOptions);
#  createNew @5 (createNew :Bool) -> (openOptions :OpenOptions);
#}

interface DirEntry {
  open @0 () -> (file :File);
  openWith @1 (openOptions :OpenOptions) -> (file :File);
  openDir @2 () -> (dir :Dir);
  removeFile @3 () -> ();
  removeDir @4 () -> ();
  metadata @5 () -> (metadata :Metadata);
  fileType @6 () -> (type :FileType);
  fileName @7 () -> (result :Text);
}

interface AmbientAuthority {
  fileOpenAmbient @0 (path :Text) -> (file :File);
  fileCreateAmbient @1 (path :Text) -> (file :File);
  fileOpenAmbientWith @2 (path :Text, openOptions :OpenOptions) -> (file :File);
  dirOpenAmbient @3 (path :Text) -> (result :Dir);
  dirOpenParent @4 () -> (result :Dir);
  dirCreateAmbientAll @5 (path :Text) -> ();
  monotonicClockNew @6 () -> (clock :MonotonicClock);
  systemClockNew @7 () -> (clock :SystemClock);
  projectDirsFrom @8 (qualifier :Text, organization :Text, application :Text) -> (projectDirs :ProjectDirs);
  userDirsHomeDir @9 () -> (dir :Dir);
  userDirsAudioDir @10 () -> (dir :Dir);
  userDirsDesktopDir @11 () -> (dir :Dir);
  userDirsDocumentDir @12 () -> (dir :Dir);
  userDirsDownloadDir @13 () -> (dir :Dir);
  userDirsFontDir @14 () -> (dir :Dir);
  userDirsPictureDir @15 () -> (dir :Dir);
  userDirsPublicDir @16 () -> (dir :Dir);
  userDirsTemplateDir @17 () -> (dir :Dir);
  userDirsVideoDir @18 () -> (dir :Dir);
  tempDirNew @19 () -> (tempDir :TempDir);
}

interface ProjectDirs {
  cacheDir @0 () -> (dir :Dir);
  configDir @1 () -> (dir :Dir);
  dataDir @2 () -> (dir :Dir);
  dataLocalDir @3 () -> (dir :Dir);
  runtimeDir @4 () -> (dir :Dir);
}

interface UserDirs {
  new @0 () -> (userDirs :UserDirs);
}

interface TempDir extends(Dir) {
  close @0 () -> ();
}

interface TempFile {
  asFile @0 () -> (file :File);
  replace @1 (dest :Text) -> ();
}