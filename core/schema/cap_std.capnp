@0xd5b73ad4256c863f;

interface CapFs {
  useAmbientAuthority @0 () -> (ambientAuthority :AmbientAuthority);
  dirOpen @1 (path :Text) -> (dir :Dir);
  dirBuilderNew @2 () -> (builder :DirBuilder);
  options @3 () -> (options :OpenOptions);
  #tempDirNewIn @4 (dir :Dir) -> (tempDir :TempDir);
  #tempFileNew @5 (dir :Dir) -> (tempFile :TempFile);
  #tempFileNewAnonymous @6 (dir :Dir) -> (file :File);
}

using Stream = import "std/byte_stream.capnp".ByteStream;

interface File {
  syncAll @0 () -> ();
  syncData @1 () -> ();
  setLen @2 (size :UInt64) -> ();
  metadata @3 () -> (metadata :Metadata);
  tryClone @4 () -> (cloned :File);
  setPermissions @5 (perm :Permissions) -> ();
  open @6 () -> (stream :Stream);
}

interface Dir {
  open @0 (path :Text) -> (file :File);
  openWith @1 (path :Text, options :OpenOptions) -> (file :File);
  createDir @2 (path :Text) -> ();
  createDirAll @3 (path :Text) -> ();
  createDirWith @4 (path :Text, dirBuilder :DirBuilder) -> ();
  create @5 (path :Text) -> (file :File);
  canonicalize @6 (path :Text) -> (pathBuf :Text);
  copy @7 (pathFrom :Text, pathTo :Text) -> (result :UInt64);
  hardLink @8 (srcPath :Text, dstPath :Text) -> ();
  metadata @9 (path :Text) -> (metadata :Metadata);
  dirMetadata @10 () -> (metadata :Metadata);
  entries @11 () -> (iter :ReadDir);
  readDir @12 (path :Text) -> (iter :ReadDir);
  read @13 (path :Text) -> (result :Data);
  readLink @14 (path :Text) -> (result :Text);
  readToString @15 (path :Text) -> (result :Text);
  removeDir @16 (path :Text) -> ();
  removeDirAll @17 (path :Text) -> ();
  removeOpenDir @18 () -> ();
  removeOpenDirAll @19 () -> ();
  removeFile @20 (path :Text) -> ();
  rename @21 (from :Text, to :Text) -> ();
  setPermissions @22 (path :Text, perm :Permissions) -> ();
  symlinkMetadata @23 (path :Text) -> (metadata :Metadata);
  write @24 (path :Text, contents :Data) -> ();
  symlink @25 (original :Text, link :Text) -> ();

  #Several unimplemented unix functions in cap std
  
  #tryClone @26 () -> (directory :Directory);
  exists @26 (path :Text) -> (result :Bool);
  tryExists @27 (path :Text) -> (result :Bool);
  isFile @28 (path :Text) -> (result :Bool);
  isDir @29 (path :Text) -> (result :Bool);
  tempDirNewIn @30 () -> (tempDir :TempDir);
  tempFileNew @31 () -> (tempFile :TempFile);
  tempFileNewAnonymous @32 () -> (file :File);
  #reopenDir @31 (dir :Filelike) -> (result :Dir); #Filelike = primitive type trait thing
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
  durationSince @0 (earlier :Instant) -> (duration :Duration);
  checkedDurationSince @1 (earlier :Instant) -> (duration :Duration);
  saturatingDurationSince @2 (earlier :Instant) -> (duration :Duration);
  checkedAdd @3 (duration :Duration) -> (instant :Instant);
  checkedSub @4 (duration :Duration) -> (instant :Instant);
}

interface MonotonicClock {
  now @0 () -> (instant :Instant);
  elapsed @1 (instant :Instant) -> (duration :Duration);
}

interface SystemClock {
  now @0 () -> (time :SystemTime);
  struct ElapsedResult {
    union {
      duration @0 :Duration;
      error @1 :SystemTimeError;
    }
  }
  elapsed @1 (systemTime :SystemTime) -> (result :ElapsedResult);
}

interface SystemTime {
  durationSince @0 (earlier :SystemTime) -> (duration :Duration);
  checkedAdd @1 (duration :Duration) -> (result :SystemTime);
  checkedSub @2 (duration :Duration) -> (result :SystemTime);
}

interface SystemTimeError {
  duration @0 () -> (duration :Duration);
}

interface DirBuilder {
  recursive @0 (recursive :Bool) -> (builder :DirBuilder);
  options @1 () -> (options :DirOptions);
  isRecursive @2 () -> (result :Bool);
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

interface OpenOptions {
  read @0 (read :Bool) -> (openOptions :OpenOptions);
  write @1 (write :Bool) -> (openOptions :OpenOptions);
  append @2 (append :Bool) -> (openOptions :OpenOptions);
  truncate @3 (truncate :Bool) -> (openOptions :OpenOptions);
  create @4 (create :Bool) -> (openOptions :OpenOptions);
  createNew @5 (createNew :Bool) -> (openOptions :OpenOptions);
}

interface DirOptions {

}

interface DirEntry {
  open @0 () -> (file :File);
  openWith @1 (options :OpenOptions) -> (file :File);
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
  fileOpenAmbientWith @2 (path :Text, options :OpenOptions) -> (file :File);
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
  asFileMut @1 () -> (file :File);
  replace @2 (dest :Text) -> ();
}