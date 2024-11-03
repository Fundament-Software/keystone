use std::borrow::Borrow;
use std::{cell::RefCell, io::Write, rc::Rc};

use crate::{
    byte_stream::ByteStreamImpl,
    cap_std_capnp::{
        ambient_authority, dir, dir_entry, file, instant, metadata, monotonic_clock, permissions,
        project_dirs, read_dir, system_clock, system_time, temp_dir, temp_file, FileType,
    },
};
use cap_directories::{self, ProjectDirs, UserDirs};
use cap_std::{
    fs::{Dir, DirEntry, File, Metadata, OpenOptions, Permissions, ReadDir},
    io_lifetimes::raw::AsRawFilelike,
    time::{Duration, Instant, MonotonicClock, SystemClock, SystemTime},
    AmbientAuthority,
};
use cap_tempfile::{TempDir, TempFile};
use capnp::{capability::Promise, Error};
use capnp_macros::{capnp_let, capnproto_rpc};
use capnp_rpc::CapabilityServerSet;

pub struct AmbientAuthorityImpl {
    pub authority: AmbientAuthority,
    pub file_set: CapabilityServerSet<FileImpl, file::Client>,
    pub dir_set: CapabilityServerSet<DirImpl, dir::Client>,
    pub instant_set: CapabilityServerSet<InstantImpl, instant::Client>,
}

impl AmbientAuthorityImpl {
    pub fn new() -> Self {
        Default::default()
    }

    pub async fn get_file_handle(&self, client: &file::Client) -> Option<u64> {
        Some(
            self.file_set
                .get_local_server(client)
                .await?
                .file
                .as_raw_filelike() as u64,
        )
    }

    pub fn new_file(this: &Rc<RefCell<Self>>, file: File) -> file::Client {
        this.borrow_mut().file_set.new_client(FileImpl {
            file,
            ambient: this.clone(),
        })
    }

    pub fn new_dir(this: &Rc<RefCell<Self>>, dir: Dir) -> dir::Client {
        this.borrow_mut().dir_set.new_client(DirImpl {
            dir,
            ambient: this.clone(),
        })
    }

    pub fn new_instant(this: &Rc<RefCell<Self>>, instant: Instant) -> instant::Client {
        this.borrow_mut().instant_set.new_client(InstantImpl {
            instant,
            ambient: this.clone(),
        })
    }
}

impl Default for AmbientAuthorityImpl {
    fn default() -> Self {
        Self {
            authority: cap_std::ambient_authority(),
            file_set: CapabilityServerSet::new(),
            dir_set: CapabilityServerSet::new(),
            instant_set: CapabilityServerSet::new(),
        }
    }
}

#[capnproto_rpc(ambient_authority)]
impl ambient_authority::Server for Rc<RefCell<AmbientAuthorityImpl>> {
    async fn file_open_ambient(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_file) = File::open_ambient(path, self.as_ref().borrow().authority) else {
            return Err(Error::failed(
                "Failed to open file using ambient authority".into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(self, _file));
        Ok(())
    }
    async fn file_create_ambient(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_file) = File::create_ambient(path, self.as_ref().borrow().authority) else {
            return Err(Error::failed(
                "Failed to create file using ambient authority".into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(self, _file));
        Ok(())
    }
    async fn file_open_ambient_with(&self, path: Reader, open_options: Reader) {
        capnp_let!({read, write, append, truncate, create, create_new} = open_options);
        let mut options = OpenOptions::new();
        let path = path.to_str()?;
        options
            .read(read)
            .write(write)
            .append(append)
            .truncate(truncate)
            .create(create)
            .create_new(create_new);
        let Ok(_file) = File::open_ambient_with(path, &options, self.as_ref().borrow().authority)
        else {
            return Err(Error::failed(
                "Failed to open file for reading(With custom options) using ambient authority"
                    .into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(self, _file));
        Ok(())
    }
    async fn dir_open_ambient(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_dir) = Dir::open_ambient_dir(path, self.as_ref().borrow().authority) else {
            return Err(Error::failed(
                "Failed to open dir using ambient authority".into(),
            ));
        };
        results
            .get()
            .set_result(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn dir_open_parent(&self, dir: Dir) {
        let Some(dir_impl) = self
            .as_ref()
            .borrow()
            .dir_set
            .get_local_server_of_resolved(&dir)
        else {
            return Err(Error::failed("Dir not from the same machine".into()));
        };
        let dir = &dir_impl.as_ref().borrow().dir;
        let Ok(_dir) = dir.open_parent_dir(self.as_ref().borrow().authority) else {
            return Err(Error::failed(
                "Failed to open parent dir using ambient authority".into(),
            ));
        };
        results
            .get()
            .set_result(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn dir_create_ambient_all(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(()) = Dir::create_ambient_dir_all(path, self.as_ref().borrow().authority) else {
            return Err(Error::failed(
                "Failed to recursively create all dirs using ambient authority".into(),
            ));
        };
        Ok(())
    }
    async fn monotonic_clock_new(&self) {
        results
            .get()
            .set_clock(capnp_rpc::new_client(MonotonicClockImpl {
                monotonic_clock: MonotonicClock::new(self.as_ref().borrow().authority),
                ambient: self.clone(),
            }));
        Ok(())
    }
    async fn system_clock_new(&self) {
        results
            .get()
            .set_clock(capnp_rpc::new_client(SystemClockImpl {
                system_clock: SystemClock::new(self.as_ref().borrow().authority),
            }));
        Ok(())
    }
    async fn project_dirs_from(
        &self,
        qualifier: Reader,
        organization: Reader,
        application: Reader,
    ) {
        let Some(_project_dirs) = ProjectDirs::from(
            qualifier.to_str()?,
            organization.to_str()?,
            application.to_str()?,
            self.as_ref().borrow().authority,
        ) else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        results
            .get()
            .set_project_dirs(capnp_rpc::new_client(ProjectDirsImpl {
                project_dirs: _project_dirs,
                ambient: self.clone(),
            }));
        Ok(())
    }
    async fn user_dirs_home_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::home_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open home dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_audio_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::audio_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open audio dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_desktop_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::desktop_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open desktop dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_document_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::document_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open document dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_download_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::download_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open download dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_font_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::font_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open font dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_picture_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::picture_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open picture dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_public_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::public_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open user's public dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_template_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::template_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open template dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn user_dirs_video_dir(&self) {
        let Some(user_dirs) = UserDirs::new() else {
            return Err(Error::failed("No valid $HOME directory".into()));
        };
        let Ok(_dir) = UserDirs::video_dir(&user_dirs, self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to open video dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(self, _dir));
        Ok(())
    }
    async fn temp_dir_new(&self) {
        let Ok(dir) = TempDir::new(self.as_ref().borrow().authority) else {
            return Err(Error::failed("Failed to create temp dir".into()));
        };
        results
            .get()
            .set_temp_dir(capnp_rpc::new_client(TempDirImpl {
                temp_dir: RefCell::new(Some(dir)),
                ambient: self.clone(),
            }));
        Ok(())
    }
}

pub struct DirImpl {
    pub dir: Dir,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(dir)]
impl dir::Server for DirImpl {
    async fn open(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_file) = self.dir.open(path) else {
            return Err(Error::failed("Failed to open file".into()));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }
    async fn open_with(&self, open_options: Reader, path: Reader) {
        capnp_let!({read, write, append, truncate, create, create_new} = open_options);
        let mut options = OpenOptions::new();
        let path = path.to_str()?;
        options
            .read(read)
            .write(write)
            .append(append)
            .truncate(truncate)
            .create(create)
            .create_new(create_new);
        let Ok(_file) = self.dir.open_with(path, &options) else {
            return Err(Error::failed(
                "Failed to open file for reading(With custom options)".into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }
    async fn create_dir(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_dir) = self.dir.create_dir(path) else {
            return Err(Error::failed("Failed to create dir".into()));
        };
        Ok(())
    }
    async fn create_dir_all(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_dir) = self.dir.create_dir_all(path) else {
            return Err(Error::failed("Failed to create dir(all)".into()));
        };
        Ok(())
    }
    async fn create(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_file) = self.dir.create(path) else {
            return Err(Error::failed(
                "Failed to open a file in write only mode".into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }
    async fn canonicalize(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(path_buf) = self.dir.canonicalize(path) else {
            return Err(Error::failed("Failed to canonicalize path".into()));
        };
        let Some(_str) = path_buf.to_str() else {
            return Err(Error::failed("Path contains non utf-8 characters".into()));
        };
        results.get().set_path_buf(_str.into());
        Ok(())
    }
    async fn copy(&self, path_from: Reader, path_to: Reader, dir_to: Capability) {
        let from = path_from.to_str()?;
        let to = path_to.to_str()?;
        let Some(dir_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .dir_set
            .get_local_server_of_resolved(&dir_to)
        else {
            return Err(Error::failed("Dir not from the same machine".into()));
        };
        let dir_to = &dir_impl.as_ref().borrow().dir;
        let Ok(bytes) = self.dir.copy(from, dir_to, to) else {
            return Err(Error::failed("Failed to copy file contents".into()));
        };
        results.get().set_result(bytes);
        Ok(())
    }
    async fn hard_link(&self, src_path: Reader, dst_path: Reader, dst_dir: Capability) {
        let src = src_path.to_str()?;
        let dst = dst_path.to_str()?;

        let Some(dir_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .dir_set
            .get_local_server_of_resolved(&dst_dir)
        else {
            return Err(Error::failed("Dir not from the same machine".into()));
        };
        let dst_dir = &dir_impl.as_ref().borrow().dir;
        let Ok(()) = self.dir.hard_link(src, dst_dir, dst) else {
            return Err(Error::failed("Failed to create hard link".into()));
        };
        Ok(())
    }
    async fn metadata(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_metadata) = self.dir.metadata(path) else {
            return Err(Error::failed("Failed to get file metadata".into()));
        };
        results
            .get()
            .set_metadata(capnp_rpc::new_client(MetadataImpl {
                metadata: _metadata,
            }));
        Ok(())
    }
    async fn dir_metadata(&self) {
        let Ok(_metadata) = self.dir.dir_metadata() else {
            return Err(Error::failed("Failed to get dir metadata".into()));
        };
        results
            .get()
            .set_metadata(capnp_rpc::new_client(MetadataImpl {
                metadata: _metadata,
            }));
        Ok(())
    }
    async fn entries(&self) {
        let Ok(mut _iter) = self.dir.entries() else {
            return Err(Error::failed("Failed to get dir entries".into()));
        };
        results.get().set_iter(capnp_rpc::new_client(ReadDirImpl {
            iter: RefCell::new(_iter),
            ambient: self.ambient.clone(),
        }));
        Ok(())
    }
    async fn read_dir(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(mut _iter) = self.dir.read_dir(path) else {
            return Err(Error::failed("Failed to read dir".into()));
        };
        results.get().set_iter(capnp_rpc::new_client(ReadDirImpl {
            iter: RefCell::new(_iter),
            ambient: self.ambient.clone(),
        }));
        Ok(())
    }
    async fn read(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(vec) = self.dir.read(path) else {
            return Err(Error::failed("Failed to read file".into()));
        };
        results.get().set_result(vec.as_slice());
        Ok(())
    }
    async fn read_link(&self, path: Readers) {
        let path = path.to_str()?;
        let Ok(_pathbuf) = self.dir.read_link(path) else {
            return Err(Error::failed("Failed to read link".into()));
        };
        let Some(_str) = _pathbuf.to_str() else {
            return Err(Error::failed("Failed to read link".into()));
        };
        results.get().set_result(_str.into());
        Ok(())
    }
    async fn read_to_string(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(string) = self.dir.read_to_string(path) else {
            return Err(Error::failed("Failed to read file to string".into()));
        };
        results.get().set_result(string.as_str().into());
        Ok(())
    }
    async fn remove_dir(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(()) = self.dir.remove_dir(path) else {
            return Err(Error::failed("Failed to remove dir".into()));
        };
        Ok(())
    }
    async fn remove_dir_all(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(()) = self.dir.remove_dir_all(path) else {
            return Err(Error::failed("Failed to remove dir(all)".into()));
        };
        Ok(())
    }
    async fn remove_open_dir(&self) {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Err(Error::failed("Failed to create an owned dir".into()));
        };
        let Ok(()) = this.remove_open_dir() else {
            return Err(Error::failed("Failed to remove open dir".into()));
        };
        Ok(())
    }
    async fn remove_open_dir_all(&self) {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Err(Error::failed("Failed to create an owned dir".into()));
        };
        let Ok(()) = this.remove_open_dir_all() else {
            return Err(Error::failed("Failed to remove open dir(all)".into()));
        };
        Ok(())
    }
    async fn remove_file(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(()) = self.dir.remove_file(path) else {
            return Err(Error::failed("Failed to remove file".into()));
        };
        Ok(())
    }
    async fn rename(&self, from: Reader, to: Reader) {
        let from = from.to_str()?;
        let to = to.to_str()?;
        let this = &self.dir;
        let Ok(()) = self.dir.rename(from, this, to) else {
            return Err(Error::failed("Failed to rename file".into()));
        };
        Ok(())
    }
    async fn set_readonly(&self, path: Reader, readonly: bool) {
        let path = path.to_str()?;
        let Ok(_meta) = self.dir.metadata(path) else {
            return Err(Error::failed(
                "Failed to get underlying file's metadata".into(),
            ));
        };
        let mut permissions = _meta.permissions();
        permissions.set_readonly(readonly);
        let Ok(()) = self.dir.set_permissions(path, permissions) else {
            return Err(Error::failed(
                "Failed to change permissions of the underlying file".into(),
            ));
        };
        Ok(())
    }
    async fn symlink_metadata(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_metadata) = self.dir.symlink_metadata(path) else {
            return Err(Error::failed("Failed to get symlink metadata".into()));
        };
        results
            .get()
            .set_metadata(capnp_rpc::new_client(MetadataImpl {
                metadata: _metadata,
            }));
        Ok(())
    }
    async fn write(&self, path: Reader, contents: &[u8]) {
        let path = path.to_str()?;
        let Ok(()) = self.dir.write(path, contents) else {
            return Err(Error::failed("Failed to write to file".into()));
        };
        Ok(())
    }
    async fn symlink(&self, original: Reader, link: Reader) {
        let original = original.to_str()?;
        let link = link.to_str()?;
        #[cfg(target_os = "windows")]
        let Ok(()) = self.dir.symlink_dir(original, link) else {
            return Err(Error::failed("Failed to create symlink".into()));
        };
        #[cfg(not(target_os = "windows"))]
        let Ok(()) = self.dir.symlink(original, link) else {
            return Err(Error::failed("Failed to create symlink".into()));
        };
        Ok(())
    }
    async fn exists(&self, path: Reader) {
        let path = path.to_str()?;
        let _results = self.dir.exists(path);
        results.get().set_result(_results);
        Ok(())
    }
    async fn try_exists(&self, path: Reader) {
        let path = path.to_str()?;
        let Ok(_results) = self.dir.try_exists(path) else {
            return Err(Error::failed("Failed to check if entity exists".into()));
        };
        results.get().set_result(_results);
        Ok(())
    }
    async fn is_file(&self, path: Reader) {
        let path = path.to_str()?;
        let _results = self.dir.is_file(path);
        results.get().set_result(_results);
        Ok(())
    }
    async fn is_dir(&self, path: Reader) {
        let path = path.to_str()?;
        let _results = self.dir.is_dir(path);
        results.get().set_result(_results);
        Ok(())
    }

    async fn temp_dir_new_in(&self) {
        let Ok(temp_dir) = cap_tempfile::TempDir::new_in(&self.dir) else {
            return Err(Error::failed("Failed to create temp dir".into()));
        };
        results
            .get()
            .set_temp_dir(capnp_rpc::new_client(TempDirImpl {
                temp_dir: RefCell::new(Some(temp_dir)),
                ambient: self.ambient.clone(),
            }));
        Ok(())
    } /*
      async fn temp_file_new(&self,  params: dir::TempFileNewParams, mut results: dir::TempFileNewresultss) {

          let dir_cap = pry!(params_reader.get_dir());
          let Some(underlying_dir) = DIR_SET.borrow().get_local_server_of_resolved(&dir_cap)) else {
              return Err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Dir not from the same machine")});
          };
          let dir = underlying_dir.borrow().dir.try_clone().unwrap();
          let Ok(temp_file) = cap_tempfile::TempFile::new(&dir) else {
              return Err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp file")});
          };
          results.get().set_temp_file(capnp_rpc::new_client(TempFileImpl{temp_file: Some(temp_file)}));
          Ok(())
      }*/
    async fn temp_file_new_anonymous(&self) {
        let Ok(file) = cap_tempfile::TempFile::new_anonymous(&self.dir) else {
            return Err(Error::failed("Failed to create anonymous temp file".into()));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, file));
        Ok(())
    }
}

pub struct ReadDirImpl {
    iter: RefCell<ReadDir>,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(read_dir)]
impl read_dir::Server for ReadDirImpl {
    async fn next(&self) {
        let Some(_results) = self.iter.borrow_mut().next() else {
            return Err(Error::failed("Final entry reached".into()));
        };
        let Ok(_entry) = _results else {
            return Err(Error::failed(
                "Encountered an error while getting dir entry".into(),
            ));
        };
        results.get().set_entry(capnp_rpc::new_client(DirEntryImpl {
            entry: _entry,
            ambient: self.ambient.clone(),
        }));
        Ok(())
    }
}

pub struct DirEntryImpl {
    entry: DirEntry,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}

#[capnproto_rpc(dir_entry)]
impl dir_entry::Server for DirEntryImpl {
    async fn open(&self) {
        let Ok(_file) = self.entry.open() else {
            return Err(Error::failed("Failed to open file for reading".into()));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }

    async fn open_with(&self, open_options: Reader) {
        capnp_let!({read, write, append, truncate, create, create_new} = open_options);
        let mut options = OpenOptions::new();
        options
            .read(read)
            .write(write)
            .append(append)
            .truncate(truncate)
            .create(create)
            .create_new(create_new);
        let Ok(_file) = self.entry.open_with(&options) else {
            return Err(Error::failed(
                "Failed to open file for reading(With custom options)".into(),
            ));
        };
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }
    async fn open_dir(&self) {
        let Ok(_dir) = self.entry.open_dir() else {
            return Err(Error::failed("Failed to open the entry as a dir".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
    async fn remove_file(&self) {
        let Ok(()) = self.entry.remove_file() else {
            return Err(Error::failed("Failed to remove file".into()));
        };
        Ok(())
    }
    async fn remove_dir(&self) {
        let Ok(()) = self.entry.remove_dir() else {
            return Err(Error::failed("Failed to remove dir".into()));
        };
        Ok(())
    }
    async fn metadata(&self) {
        let Ok(_metadata) = self.entry.metadata() else {
            return Err(Error::failed("Failed to get file metadata".into()));
        };
        results
            .get()
            .set_metadata(capnp_rpc::new_client(MetadataImpl {
                metadata: _metadata,
            }));
        Ok(())
    }
    async fn file_type(&self) {
        let Ok(_file_type) = self.entry.file_type() else {
            return Err(Error::failed("Failed to get file type".into()));
        };
        let _type: FileType = if _file_type.is_dir() {
            FileType::Dir
        } else if _file_type.is_file() {
            FileType::File
        } else if _file_type.is_symlink() {
            FileType::Symlink
        } else {
            return Err(Error::failed("Unknown file type".into()));
        };
        results.get().set_type(_type);
        Ok(())
    }
    async fn file_name(&self) {
        let _name = self.entry.file_name();
        let Some(_name) = _name.to_str() else {
            return Err(Error::failed("File name not valid utf-8".into()));
        };
        results.get().set_result(_name.into());
        Ok(())
    }
}

pub struct FileImpl {
    file: File,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(file)]
impl file::Server for FileImpl {
    async fn sync_all(&self) {
        let Ok(()) = self.file.sync_all() else {
            return Err(Error::failed(
                "Failed to sync os-internal metadata to disk".into(),
            ));
        };
        Ok(())
    }
    async fn sync_data(&self) {
        let Ok(()) = self.file.sync_data() else {
            return Err(Error::failed(
                "Failed to sync os-internal metadata to sync data".into(),
            ));
        };
        Ok(())
    }
    async fn set_len(&self, size: u64) {
        let Ok(()) = self.file.set_len(size) else {
            return Err(Error::failed("Failed to update the size of file".into()));
        };
        Ok(())
    }
    async fn metadata(&self) {
        let Ok(_metadata) = self.file.metadata() else {
            return Err(Error::failed("Get file metadata".into()));
        };
        results
            .get()
            .set_metadata(capnp_rpc::new_client(MetadataImpl {
                metadata: _metadata,
            }));
        Ok(())
    }
    async fn try_clone(&self) {
        let Ok(_file) = self.file.try_clone() else {
            return Err(Error::failed(
                "Failed to sync os-internal metadata to sync data".into(),
            ));
        };
        results
            .get()
            .set_cloned(AmbientAuthorityImpl::new_file(&self.ambient, _file));
        Ok(())
    }
    async fn set_readonly(&self, readonly: bool) {
        let Ok(_meta) = self.file.metadata() else {
            return Err(Error::failed("Failed to get file's metadata".into()));
        };
        let mut permissions = _meta.permissions();
        permissions.set_readonly(readonly);
        let Ok(()) = self.file.set_permissions(permissions) else {
            return Err(Error::failed(
                "Failed to change permissions of the file".into(),
            ));
        };
        Ok(())
    }

    async fn open(&self) {
        let Ok(mut this) = self.file.try_clone() else {
            return Err(Error::failed("Get owned file".into()));
        };
        let _stream = ByteStreamImpl::new(move |bytes| {
            let Ok(()) = this.write_all(bytes) else {
                return Promise::err(Error::failed("Failed to write to file".into()));
            };
            Promise::ok(())
        });
        results.get().set_stream(capnp_rpc::new_client(_stream));
        Ok(())
    }
}

pub struct MetadataImpl {
    metadata: Metadata,
}
#[capnproto_rpc(metadata)]
impl metadata::Server for MetadataImpl {
    async fn file_type(&self) {
        let _file_type = self.metadata.file_type();
        let _type: FileType = if _file_type.is_dir() {
            FileType::Dir
        } else if _file_type.is_file() {
            FileType::File
        } else if _file_type.is_symlink() {
            FileType::Symlink
        } else {
            return Err(Error::failed("Unknown file type".into()));
        };
        results.get().set_file_type(_type);
        Ok(())
    }
    async fn is_dir(&self) {
        let _results = self.metadata.is_dir();
        results.get().set_result(_results);
        Ok(())
    }
    async fn is_file(&self) {
        let _results = self.metadata.is_file();
        results.get().set_result(_results);
        Ok(())
    }
    async fn is_symlink(&self) {
        let _results = self.metadata.is_symlink();
        results.get().set_result(_results);
        Ok(())
    }
    async fn len(&self) {
        let _results = self.metadata.len();
        results.get().set_result(_results);
        Ok(())
    }
    async fn permissions(&self) {
        let _permissions = self.metadata.permissions();
        results
            .get()
            .set_permissions(capnp_rpc::new_client(PermissionsImpl {
                permissions: RefCell::new(_permissions),
            }));
        Ok(())
    }
    async fn modified(&self) {
        let Ok(_time) = self.metadata.modified() else {
            return Err(Error::failed(
                "Failed to access modified field of the metadata".into(),
            ));
        };
        results
            .get()
            .set_time(capnp_rpc::new_client(SystemTimeImpl { system_time: _time }));
        Ok(())
    }
    async fn accessed(&self) {
        let Ok(_time) = self.metadata.accessed() else {
            return Err(Error::failed(
                "Failed to access accessed field of the metadata".into(),
            ));
        };
        results
            .get()
            .set_time(capnp_rpc::new_client(SystemTimeImpl { system_time: _time }));
        Ok(())
    }
    async fn created(&self) {
        let Ok(_time) = self.metadata.created() else {
            return Err(Error::failed(
                "Failed to access created field of the metadata".into(),
            ));
        };
        results
            .get()
            .set_time(capnp_rpc::new_client(SystemTimeImpl { system_time: _time }));
        Ok(())
    }
}

pub struct PermissionsImpl {
    permissions: RefCell<Permissions>,
}
#[capnproto_rpc(permissions)]
impl permissions::Server for PermissionsImpl {
    async fn readonly(&self) {
        let _results = self.permissions.borrow().readonly();
        results.get().set_result(_results);
        Ok(())
    }
    async fn set_readonly(&self, readonly: bool) {
        self.permissions.borrow_mut().set_readonly(readonly);
        Ok(())
    }
}

pub struct TempDirImpl {
    temp_dir: RefCell<Option<TempDir>>,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(temp_dir)]
impl temp_dir::Server for TempDirImpl {
    async fn close(&self) {
        let Some(dir) = self.temp_dir.borrow_mut().take() else {
            return Err(Error::failed("Temp dir already closed".into()));
        };
        let Ok(()) = dir.close() else {
            return Err(Error::failed("Failed to close temp dir".into()));
        };
        Ok(())
    }
    async fn get_as_dir(&self) {
        let Some(dir) = self.temp_dir.take() else {
            return Err(Error::failed("Temp dir already closed".into()));
        };
        let Ok(cloned) = dir.try_clone() else {
            return Err(Error::failed(
                "Failed to get an owned version of the underlying dir".into(),
            ));
        };
        *self.temp_dir.borrow_mut() = Some(dir);
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, cloned));
        Ok(())
    }
}
pub struct TempFileImpl<'a> {
    temp_file: RefCell<Option<TempFile<'a>>>,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(temp_file)]
impl temp_file::Server for TempFileImpl<'_> {
    async fn as_file(&self) {
        let Some(_file) = self.temp_file.borrow_mut().take() else {
            return Err(Error::failed("Temp file already removed".into()));
        };
        let Ok(_cloned) = _file.as_file().try_clone() else {
            return Err(Error::failed(
                "Failed to get an owned version of the underlying file".into(),
            ));
        };
        *self.temp_file.borrow_mut() = Some(_file);
        results
            .get()
            .set_file(AmbientAuthorityImpl::new_file(&self.ambient, _cloned));
        Ok(())
    }
    async fn replace(&self, dest: Reader) {
        let dest = dest.to_str()?;
        let Some(temp_file) = self.temp_file.borrow_mut().take() else {
            return Err(Error::failed("Temp file already removed".into()));
        };
        let Ok(()) = temp_file.replace(dest) else {
            return Err(Error::failed(
                "Failed to write file to the target location".into(),
            ));
        };
        Ok(())
    }
}

pub struct SystemTimeImpl {
    system_time: SystemTime,
}
#[capnproto_rpc(system_time)]
impl system_time::Server for SystemTimeImpl {
    async fn duration_since(&self, earlier: Capability) {
        let Ok(earlier_duration_since_unix_epoch) = tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current().block_on(
                earlier
                    .get_duration_since_unix_epoch_request()
                    .send()
                    .promise,
            )
        }) else {
            return Err(Error::failed("Failed to convert to unix time".into()));
        };
        let reader = earlier_duration_since_unix_epoch.get()?;
        capnp_let!({duration : {secs, nanos}} = reader);
        //Add duration since unix epoch to unix epoch to reconstruct a system time
        let earlier =
            cap_std::time::SystemTime::from_std(std::time::UNIX_EPOCH + Duration::new(secs, nanos));
        let Ok(_duration_since) = self.system_time.duration_since(earlier) else {
            return Err(Error::failed("System time earlier than self".into()));
        };
        let mut response = results.get().init_duration();
        response.set_secs(_duration_since.as_secs());
        response.set_nanos(_duration_since.subsec_nanos());
        Ok(())
    }

    async fn checked_add(&self, duration: Reader) {
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_time) = self.system_time.checked_add(duration) else {
            return Err(Error::failed(
                "Failed to add duration to system time".into(),
            ));
        };
        results
            .get()
            .set_result(capnp_rpc::new_client(SystemTimeImpl { system_time: _time }));
        Ok(())
    }

    async fn checked_sub(&self, duration: Reader) {
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_time) = self.system_time.checked_sub(duration) else {
            return Err(Error::failed(
                "Failed to subtract duration from system time".into(),
            ));
        };
        results
            .get()
            .set_result(capnp_rpc::new_client(SystemTimeImpl { system_time: _time }));
        Ok(())
    }

    async fn get_duration_since_unix_epoch(&self) {
        let Ok(_duration) = self
            .system_time
            .into_std()
            .duration_since(std::time::UNIX_EPOCH)
        else {
            return Err(Error::failed(
                "Failed to get duration since unix epoch".into(),
            ));
        };
        let mut response = results.get().init_duration();
        response.set_secs(_duration.as_secs());
        response.set_nanos(_duration.subsec_nanos());
        Ok(())
    }
}

pub struct InstantImpl {
    instant: Instant,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(instant)]
impl instant::Server for InstantImpl {
    async fn duration_since(&self, earlier: Reader) {
        let Some(instant_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .instant_set
            .get_local_server_of_resolved(&earlier)
        else {
            return Err(Error::failed(
                "Earlier instant not from the same machine".into(),
            ));
        };
        let earlier = instant_impl.as_ref().borrow().instant;
        let dur = self.instant.duration_since(earlier);
        let mut response = results.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Ok(())
    }
    async fn checked_duration_since(&self, earlier: Capability) {
        let Some(instant_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .instant_set
            .get_local_server_of_resolved(&earlier)
        else {
            return Err(Error::failed(
                "Earlier instant not from the same machine".into(),
            ));
        };
        let earlier = instant_impl.as_ref().borrow().instant;
        let Some(dur) = self.instant.checked_duration_since(earlier) else {
            return Err(Error::failed(
                "Earlier instant not from the same machine".into(),
            ));
        };
        let mut response = results.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Ok(())
    }
    async fn saturating_duration_since(&self, earlier: Capability) {
        let Some(instant_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .instant_set
            .get_local_server_of_resolved(&earlier)
        else {
            return Err(Error::failed(
                "Earlier instant not from the same machine".into(),
            ));
        };
        let earlier = instant_impl.as_ref().borrow().instant;
        let dur = self.instant.saturating_duration_since(earlier);
        let mut response = results.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Ok(())
    }
    async fn checked_add(&self, duration: Reader) {
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_instant) = self.instant.checked_add(duration) else {
            return Err(Error::failed("Failed to add duration to instant".into()));
        };
        results
            .get()
            .set_instant(AmbientAuthorityImpl::new_instant(&self.ambient, _instant));
        Ok(())
    }
    async fn checked_sub(&self, duration: Reader) {
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_instant) = self.instant.checked_sub(duration) else {
            return Err(Error::failed(
                "Failed to subtract duration from instant".into(),
            ));
        };
        results
            .get()
            .set_instant(AmbientAuthorityImpl::new_instant(&self.ambient, _instant));
        Ok(())
    }
}

pub struct MonotonicClockImpl {
    monotonic_clock: MonotonicClock,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(monotonic_clock)]
impl monotonic_clock::Server for MonotonicClockImpl {
    async fn now(&self) {
        let _instant = self.monotonic_clock.now();
        results
            .get()
            .set_instant(AmbientAuthorityImpl::new_instant(&self.ambient, _instant));
        Ok(())
    }
    async fn elapsed(&self, instant: Capability) {
        let Some(instant_impl) = self
            .ambient
            .as_ref()
            .borrow()
            .instant_set
            .get_local_server_of_resolved(&instant)
        else {
            return Err(Error::failed(
                "Earlier instant not from the same machine".into(),
            ));
        };
        let instant = instant_impl.as_ref().borrow().instant;
        let dur = self.monotonic_clock.elapsed(instant);
        let mut response = results.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Ok(())
    }
}

pub struct SystemClockImpl {
    system_clock: SystemClock,
}
#[capnproto_rpc(system_clock)]
impl system_clock::Server for SystemClockImpl {
    async fn now(&self) {
        let _system_time = self.system_clock.now();
        results
            .get()
            .set_time(capnp_rpc::new_client(SystemTimeImpl {
                system_time: _system_time,
            }));
        Ok(())
    }
    async fn elapsed(&self, duration_since_unix_epoch: Reader) {
        capnp_let!({secs, nanos} = duration_since_unix_epoch);
        //Add duration since unix epoch to unix epoch to reconstruct a system time
        let earlier =
            cap_std::time::SystemTime::from_std(std::time::UNIX_EPOCH + Duration::new(secs, nanos));
        let Ok(_elapsed) = self.system_clock.elapsed(earlier) else {
            return Err(Error::failed("Failed to get amount of time elapsed".into()));
        };
        let mut response = results.get().init_result();
        response.set_secs(_elapsed.as_secs());
        response.set_nanos(_elapsed.subsec_nanos());
        Ok(())
    }
}

pub struct ProjectDirsImpl {
    project_dirs: ProjectDirs,
    ambient: Rc<RefCell<AmbientAuthorityImpl>>,
}
#[capnproto_rpc(project_dirs)]
impl project_dirs::Server for ProjectDirsImpl {
    async fn cache_dir(&self) {
        let Ok(_dir) = self.project_dirs.cache_dir() else {
            return Err(Error::failed("Failed to retrieve cache directory".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
    async fn config_dir(&self) {
        let Ok(_dir) = self.project_dirs.config_dir() else {
            return Err(Error::failed("Failed to retrieve config directory".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
    async fn data_dir(&self) {
        let Ok(_dir) = self.project_dirs.data_dir() else {
            return Err(Error::failed("Failed to retrieve data directory".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
    async fn data_local_dir(&self) {
        let Ok(_dir) = self.project_dirs.data_local_dir() else {
            return Err(Error::failed(
                "Failed to retrieve local data directory".into(),
            ));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
    async fn runtime_dir(&self) {
        let Ok(_dir) = self.project_dirs.runtime_dir() else {
            return Err(Error::failed("Failed to retrieve runtime directory".into()));
        };
        results
            .get()
            .set_dir(AmbientAuthorityImpl::new_dir(&self.ambient, _dir));
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use crate::{
        cap_std_capnp::{ambient_authority, dir_entry, metadata, permissions, system_time},
        cap_std_capnproto::AmbientAuthorityImpl,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    #[tokio::test]
    async fn create_dir_all_canonicalize_test() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let path = std::env::temp_dir();
        open_ambient_request
            .get()
            .set_path(path.to_str().unwrap().into());
        let dir = open_ambient_request
            .send()
            .promise
            .await?
            .get()?
            .get_result()?;

        let mut create_dir_all_request = dir.create_dir_all_request();
        create_dir_all_request
            .get()
            .set_path("test_dir/testing_recursively_creating_dirs".into());
        create_dir_all_request.send().promise.await?;

        let mut canonicalize_request = dir.canonicalize_request();
        canonicalize_request
            .get()
            .set_path("test_dir/testing_recursively_creating_dirs".into());
        let results = canonicalize_request.send().promise.await?;
        let p = results.get()?.get_path_buf()?.to_str()?;
        println!("path = {p}");
        return Ok(());
    }

    #[tokio::test]
    async fn test_create_write_getmetadata() -> eyre::Result<()> {
        //use ambient authority to open a dir, create a file(Or open it in write mode if it already exists), open a bytestream, use the bytestream to write some bytes, get file metadata
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let path = std::env::temp_dir();
        open_ambient_request
            .get()
            .set_path(path.to_str().unwrap().into());
        let dir = open_ambient_request
            .send()
            .promise
            .await?
            .get()?
            .get_result()?;

        let mut create_request = dir.create_request();
        create_request.get().set_path("capnp_test.txt".into());
        let file = create_request.send().promise.await?.get()?.get_file()?;

        let open_bytestream_request = file.open_request();
        let stream = open_bytestream_request
            .send()
            .promise
            .await?
            .get()?
            .get_stream()?;

        let mut write_request = stream.write_request();
        write_request.get().set_bytes(b"Writing some bytes test ");
        let _res = write_request.send().promise.await?;

        let mut file_metadata_request = dir.metadata_request();
        file_metadata_request
            .get()
            .set_path("capnp_test.txt".into());
        let metadata = file_metadata_request
            .send()
            .promise
            .await?
            .get()?
            .get_metadata()?;
        println!("File metadata:");
        test_metadata(metadata).await?;

        let dir_metadata_request = dir.dir_metadata_request();
        let metadata = dir_metadata_request
            .send()
            .promise
            .await?
            .get()?
            .get_metadata()?;
        println!("Dir metadata:");
        test_metadata(metadata).await?;

        return Ok(());
    }

    #[tokio::test]
    async fn test_home_dir() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let home_dir_request = ambient_authority.user_dirs_home_dir_request();
        let _home_dir = home_dir_request.send().promise.await?.get()?.get_dir()?;
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn test_user_dirs() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let audio_dir_request = ambient_authority.user_dirs_audio_dir_request();
        let _audio_dir = audio_dir_request.send().promise.await?.get()?.get_dir()?;

        let desktop_dir_request = ambient_authority.user_dirs_desktop_dir_request();
        let _desktop_dir = desktop_dir_request.send().promise.await?.get()?.get_dir()?;

        let document_dir_request = ambient_authority.user_dirs_document_dir_request();
        let _document_dir = document_dir_request
            .send()
            .promise
            .await?
            .get()?
            .get_dir()?;

        let download_dir_request = ambient_authority.user_dirs_download_dir_request();
        let _download_dir = download_dir_request
            .send()
            .promise
            .await?
            .get()?
            .get_dir()?;

        #[cfg(not(target_os = "windows"))]
        let font_dir_request = ambient_authority.user_dirs_font_dir_request();
        #[cfg(not(target_os = "windows"))]
        let font_dir = font_dir_request.send().promise.await?.get()?.get_dir()?;

        let picture_dir_request = ambient_authority.user_dirs_picture_dir_request();
        let _picture_dir = picture_dir_request.send().promise.await?.get()?.get_dir()?;

        let public_dir_request = ambient_authority.user_dirs_public_dir_request();
        let _public_dir = public_dir_request.send().promise.await?.get()?.get_dir()?;

        let template_dir_request = ambient_authority.user_dirs_template_dir_request();
        let _template_dir = template_dir_request
            .send()
            .promise
            .await?
            .get()?
            .get_dir()?;

        let video_dir_request = ambient_authority.user_dirs_video_dir_request();
        let _video_dir = video_dir_request.send().promise.await?.get()?.get_dir()?;

        return Ok(());
    }

    #[tokio::test]
    async fn test_project_dirs() -> eyre::Result<()> {
        //TODO maybe create some form of generic "dir" test
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let mut project_dirs_from_request = ambient_authority.project_dirs_from_request();
        let mut project_dirs_builder = project_dirs_from_request.get();
        project_dirs_builder.set_qualifier("".into());
        project_dirs_builder.set_organization("Fundament software".into());
        project_dirs_builder.set_application("Keystone".into());
        let project_dirs = project_dirs_from_request
            .send()
            .promise
            .await?
            .get()?
            .get_project_dirs()?;

        let cache_dir_request = project_dirs.cache_dir_request();
        let _cache_dir = cache_dir_request.send().promise.await?.get()?.get_dir()?;

        let config_dir_request = project_dirs.config_dir_request();
        let _config_dir = config_dir_request.send().promise.await?.get()?.get_dir()?;

        let data_dir_request = project_dirs.data_dir_request();
        let _data_dir = data_dir_request.send().promise.await?.get()?.get_dir()?;

        let data_local_dir_request = project_dirs.data_local_dir_request();
        let _data_local_dir = data_local_dir_request
            .send()
            .promise
            .await?
            .get()?
            .get_dir()?;

        //TODO Runtime directory seems to not exist on
        #[cfg(not(target_os = "windows"))]
        let runtime_dir_request = project_dirs.runtime_dir_request();
        #[cfg(not(target_os = "windows"))]
        let runtime_dir = runtime_dir_request.send().promise.await?.get()?.get_dir()?;

        return Ok(());
    }

    #[tokio::test]
    async fn test_system_clock() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let system_clock_request = ambient_authority.system_clock_new_request();
        let system_clock = system_clock_request
            .send()
            .promise
            .await?
            .get()?
            .get_clock()?;

        let now_request = system_clock.now_request();
        let now = now_request.send().promise.await?.get()?.get_time()?;

        let duration_since_unix_epoch_request = now.get_duration_since_unix_epoch_request();
        let results = duration_since_unix_epoch_request.send().promise.await?;
        let duration_since_unix_epoch = results.get()?.get_duration()?;
        let secs = duration_since_unix_epoch.get_secs();
        let nanos = duration_since_unix_epoch.get_nanos();
        print!("\nDuration since unix epoch to now: secs:{secs} nanos:{nanos}");

        print!(" waiting 2 seconds ");
        std::thread::sleep(std::time::Duration::from_secs(2));

        let mut elapsed_request = system_clock.elapsed_request();
        let mut dur_param = elapsed_request.get().init_duration_since_unix_epoch();
        dur_param.set_secs(secs);
        dur_param.set_nanos(nanos);
        let results = elapsed_request.send().promise.await?;
        let elapsed = results.get()?.get_result()?;
        let secs = elapsed.get_secs();
        let nanos = elapsed.get_nanos();
        print!("elapsed since last: secs:{secs} nanos{nanos}\n");

        return Ok(());
    }

    #[tokio::test]
    async fn test_read_dir_iterator() -> eyre::Result<()> {
        let mut path = std::env::temp_dir();
        path.push("capnp_test_dir");
        std::fs::create_dir_all(path.clone())?;
        let mut fp = path.clone();
        fp.push("test.txt");
        let _f = std::fs::File::create(fp)?;
        let mut dp = path.clone();
        dp.push("dir1");
        std::fs::create_dir_all(dp)?;
        path.push("dir2");
        std::fs::create_dir_all(path)?;

        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        path.push("capnp_test_dir");
        open_ambient_request
            .get()
            .set_path(path.to_str().unwrap().into());
        let dir = open_ambient_request
            .send()
            .promise
            .await?
            .get()?
            .get_result()?;

        let entries_request = dir.entries_request();
        let iter = entries_request.send().promise.await?.get()?.get_iter()?;
        loop {
            match iter.next_request().send().promise.await {
                Ok(results) => {
                    println!("New entry:");
                    let entry = results.get()?.get_entry()?;
                    dir_entry_test(entry).await?;
                }
                Err(_results) => {
                    println!("Final entry reached");
                    break;
                }
            }
        }
        return Ok(());
    }

    async fn dir_entry_test(entry: dir_entry::Client) -> eyre::Result<()> {
        println!("\nDir entry test:");
        match entry.open_request().send().promise.await {
            Ok(results) => {
                println!("Is file");
                let _file = results.get()?.get_file()?;
            }
            Err(_results) => println!("Isn't file"),
        }

        match entry.open_dir_request().send().promise.await {
            Ok(results) => {
                println!("Is dir");
                let _dir = results.get()?.get_dir()?;
            }
            Err(_results) => println!("Isn't dir"),
        }

        let metadata = entry
            .metadata_request()
            .send()
            .promise
            .await?
            .get()?
            .get_metadata()?;
        test_metadata(metadata).await?;

        let file_type = entry
            .file_type_request()
            .send()
            .promise
            .await?
            .get()?
            .get_type()?;
        println!("File type = {:?}", file_type);

        let results = entry.file_name_request().send().promise.await?;
        let name = results.get()?.get_result()?.to_str()?;
        println!("File/dir name: {name}");
        return Ok(());
    }

    pub async fn test_metadata(metadata: metadata::Client) -> eyre::Result<()> {
        println!("\nMetadata test:");
        let is_dir_request = metadata.is_dir_request();
        let results = is_dir_request.send().promise.await?.get()?.get_result();
        println!("Is dir: {results}");

        let is_file_request = metadata.is_file_request();
        let results = is_file_request.send().promise.await?.get()?.get_result();
        println!("Is file: {results}");

        let is_symlink_request = metadata.is_symlink_request();
        let results = is_symlink_request.send().promise.await?.get()?.get_result();
        println!("Is symlink: {results}");

        let len_request = metadata.len_request();
        let results = len_request.send().promise.await?.get()?.get_result();
        println!("Len: {results}");

        let file_type_request = metadata.file_type_request();
        let file_type = file_type_request
            .send()
            .promise
            .await?
            .get()?
            .get_file_type()?;
        println!("File type = {:?}", file_type);

        let permissions_request = metadata.permissions_request();
        let permissions = permissions_request
            .send()
            .promise
            .await?
            .get()?
            .get_permissions()?;
        test_permissions(permissions).await?;

        let modified_request = metadata.modified_request();
        let modified_time = modified_request.send().promise.await?.get()?.get_time()?;
        println!("Modified time:");
        test_system_time(modified_time).await?;

        let accessed_request = metadata.accessed_request();
        let accessed_time = accessed_request.send().promise.await?.get()?.get_time()?;
        println!("Accessed time:");
        test_system_time(accessed_time).await?;

        let created_request = metadata.created_request();
        let created_time = created_request.send().promise.await?.get()?.get_time()?;
        println!("Created time:");
        test_system_time(created_time).await?;

        return Ok(());
    }

    async fn test_permissions(permissions: permissions::Client) -> eyre::Result<()> {
        println!("\nPermissions test:");
        let readonly_request = permissions.readonly_request();
        let results = readonly_request.send().promise.await?.get()?.get_result();
        println!("Is readonly: {results}");

        //TODO test setting readonly
        return Ok(());
    }

    async fn test_system_time(time: system_time::Client) -> eyre::Result<()> {
        println!("\nSystem time test:");
        //TODO test other stuff

        let get_duration_since_unix_epoch_request = time.get_duration_since_unix_epoch_request();
        let results = get_duration_since_unix_epoch_request.send().promise.await?;
        let duration = results.get()?.get_duration()?;
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        //capnp_let!({secs, nanos} = duration);
        println!("Duration since unix epoch: secs:{secs} nanos:{nanos}");
        return Ok(());
    }

    #[tokio::test]
    async fn test_open_read() -> eyre::Result<()> {
        //use ambient authority to open directory, read contents of a file as bytes and print them out
        use std::io::{BufWriter, Write};

        let mut path = std::env::temp_dir();
        path.push("capnp_test.txt");
        let _f = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(_f);
        writer.write_all(b"Just a test file ")?;
        writer.flush()?;

        let ambient_authority: ambient_authority::Client =
            capnp_rpc::new_client(Rc::new(RefCell::new(AmbientAuthorityImpl::new())));

        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let path = std::env::temp_dir();
        open_ambient_request
            .get()
            .set_path(path.to_str().unwrap().into());
        let dir = open_ambient_request
            .send()
            .promise
            .await?
            .get()?
            .get_result()?;

        let mut read_request = dir.read_request();
        read_request.get().set_path("capnp_test.txt".into());
        let res = read_request.send().promise.await?;
        let out = res.get()?.get_result()?;
        for c in out {
            print!("{}", *c as char)
        }
        return Ok(());
    }
}
