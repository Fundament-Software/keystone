use std::{path::Path, io::Write, cell::RefCell};

use cap_std::{fs::{Dir, DirBuilder, File, Metadata, ReadDir, Permissions, OpenOptions, DirEntry}, time::{MonotonicClock, SystemClock, SystemTime, Duration, Instant}};
use cap_tempfile::{TempFile, TempDir};
use cap_directories::{self, UserDirs, ProjectDirs};
use capnp::{capability::Promise, Error, traits::IntoInternalStructReader};
use capnp_rpc::{pry, CapabilityServerSet};
use capnp_macros::capnp_let;
use tokio::io::{AsyncRead, AsyncReadExt};
use crate::{cap_std_capnp::{ambient_authority, dir, dir_entry, duration, file, FileType, instant, metadata, monotonic_clock, open_options, permissions, project_dirs, read_dir, system_clock, system_time, temp_dir, temp_file, user_dirs}, spawn::unix_process::UnixProcessServiceSpawnImpl, byte_stream::ByteStreamImpl};
use capnp::IntoResult;

thread_local! (
    static DIR_SET: RefCell<CapabilityServerSet<DirImpl, dir::Client>> = RefCell::new(CapabilityServerSet::new());
    static INSTANT_SET: RefCell<CapabilityServerSet<InstantImpl, instant::Client>> = RefCell::new(CapabilityServerSet::new());
);

pub struct AmbientAuthorityImpl;

impl ambient_authority::Server for AmbientAuthorityImpl {
    fn file_open_ambient(&mut self, params: ambient_authority::FileOpenAmbientParams, mut result: ambient_authority::FileOpenAmbientResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_file) = File::open_ambient(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file using ambient authority")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn file_create_ambient(&mut self, params: ambient_authority::FileCreateAmbientParams, mut result: ambient_authority::FileCreateAmbientResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_file) = File::create_ambient(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create file using ambient authority")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn file_open_ambient_with(&mut self, params: ambient_authority::FileOpenAmbientWithParams, mut result: ambient_authority::FileOpenAmbientWithResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        capnp_let!({open_options : {read, write, append, truncate, create, create_new}} = params_reader);
        let mut options = OpenOptions::new();
        let path = pry!(pry!(params_reader.get_path()).to_str());
        options.read(read).write(write).append(append).truncate(truncate).create(create).create_new(create_new);
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_file) = File::open_ambient_with(path, &options, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file for reading(With custom options) using ambient authority")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn dir_open_ambient(&mut self, params: ambient_authority::DirOpenAmbientParams, mut result: ambient_authority::DirOpenAmbientResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_dir) = Dir::open_ambient_dir(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open dir using ambient authority")});
        };
        result.get().set_result(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn dir_open_parent(&mut self, params: ambient_authority::DirOpenParentParams, mut result: ambient_authority::DirOpenParentResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let dir_cap = pry!(params_reader.get_dir());
        let Some(dir_impl) = DIR_SET.with_borrow(|set| set.get_local_server_of_resolved(&dir_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Dir not from the same machine")});
        };
        let ambient_authority = cap_std::ambient_authority();
        let dir = &dir_impl.borrow().dir;
        let Ok(_dir) = dir.open_parent_dir(ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open parent dir using ambient authority")});
        };
        result.get().set_result(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn dir_create_ambient_all(&mut self, params: ambient_authority::DirCreateAmbientAllParams, mut result: ambient_authority::DirCreateAmbientAllResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(()) = Dir::create_ambient_dir_all(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to recursively create all dirs using ambient authority")});
        };
        Promise::ok(())
    }
    fn monotonic_clock_new(&mut self, _: ambient_authority::MonotonicClockNewParams, mut result: ambient_authority::MonotonicClockNewResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        result.get().set_clock(capnp_rpc::new_client(MonotonicClockImpl{monotonic_clock: MonotonicClock::new(ambient_authority)}));
        Promise::ok(())
    }
    fn system_clock_new(&mut self, _: ambient_authority::SystemClockNewParams, mut result: ambient_authority::SystemClockNewResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        result.get().set_clock(capnp_rpc::new_client(SystemClockImpl{system_clock: SystemClock::new(ambient_authority)}));
        Promise::ok(())
    }
    fn project_dirs_from(&mut self, params: ambient_authority::ProjectDirsFromParams, mut result: ambient_authority::ProjectDirsFromResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        capnp_let!({qualifier, organization, application} = params_reader);
        let ambient_authority = cap_std::ambient_authority();
        let Some(_project_dirs) = ProjectDirs::from(pry!(qualifier.to_str()), pry!(organization.to_str()), pry!(application.to_str()), ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        result.get().set_project_dirs(capnp_rpc::new_client(ProjectDirsImpl{project_dirs: _project_dirs}));
        Promise::ok(())
    }
    fn user_dirs_home_dir(&mut self, _: ambient_authority::UserDirsHomeDirParams, mut result: ambient_authority::UserDirsHomeDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::home_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open home dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_audio_dir(&mut self, _: ambient_authority::UserDirsAudioDirParams, mut result: ambient_authority::UserDirsAudioDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::audio_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open audio dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_desktop_dir(&mut self, _: ambient_authority::UserDirsDesktopDirParams, mut result: ambient_authority::UserDirsDesktopDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::desktop_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open desktop dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_document_dir(&mut self, _: ambient_authority::UserDirsDocumentDirParams, mut result: ambient_authority::UserDirsDocumentDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::document_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open document dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_download_dir(&mut self, _: ambient_authority::UserDirsDownloadDirParams, mut result: ambient_authority::UserDirsDownloadDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::download_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open download dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_font_dir(&mut self, _: ambient_authority::UserDirsFontDirParams, mut result: ambient_authority::UserDirsFontDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::font_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open font dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_picture_dir(&mut self, _: ambient_authority::UserDirsPictureDirParams, mut result: ambient_authority::UserDirsPictureDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::picture_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open picture dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_public_dir(&mut self, _: ambient_authority::UserDirsPublicDirParams, mut result: ambient_authority::UserDirsPublicDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::public_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open user's public dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_template_dir(&mut self, _: ambient_authority::UserDirsTemplateDirParams, mut result: ambient_authority::UserDirsTemplateDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::template_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open template dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn user_dirs_video_dir(&mut self, _: ambient_authority::UserDirsVideoDirParams, mut result: ambient_authority::UserDirsVideoDirResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Some(user_dirs) = UserDirs::new() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("No valid $HOME directory")});
        };
        let Ok(_dir) = UserDirs::video_dir(&user_dirs, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open video dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn temp_dir_new(&mut self, _: ambient_authority::TempDirNewParams, mut result: ambient_authority::TempDirNewResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Ok(dir) = TempDir::new(ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp dir")});
        };
        result.get().set_temp_dir(capnp_rpc::new_client(TempDirImpl{temp_dir: Some(dir)}));
        Promise::ok(())
    }
    
}

pub struct DirImpl {
    pub dir: Dir
}

impl dir::Server for DirImpl { 
    fn open(&mut self, params: dir::OpenParams, mut result: dir::OpenResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(pry!(path_reader.get_path()).to_str());
        let Ok(_file) = self.dir.open(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn open_with(&mut self, params: dir::OpenWithParams, mut result: dir::OpenWithResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        capnp_let!({open_options : {read, write, append, truncate, create, create_new}} = params_reader);
        let mut options = OpenOptions::new();
        let path = pry!(pry!(params_reader.get_path()).to_str());
        options.read(read).write(write).append(append).truncate(truncate).create(create).create_new(create_new);
        let Ok(_file) = self.dir.open_with(path, &options) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file for reading(With custom options)")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn create_dir(&mut self, params: dir::CreateDirParams, _: dir::CreateDirResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(pry!(path_reader.get_path()).to_str());
        let Ok(_dir) = self.dir.create_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create dir")});
        };
        Promise::ok(())
    }
    fn create_dir_all(&mut self, params: dir::CreateDirAllParams, _: dir::CreateDirAllResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(pry!(path_reader.get_path()).to_str());
        let Ok(_dir) = self.dir.create_dir_all(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create dir(all)")});
        };
        Promise::ok(())
    }
    fn create(&mut self, params: dir::CreateParams, mut result: dir::CreateResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(pry!(path_reader.get_path()).to_str());
        let Ok(_file) = self.dir.create(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open a file in write only mode")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn canonicalize(&mut self, params: dir::CanonicalizeParams, mut result: dir::CanonicalizeResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(pry!(path_reader.get_path()).to_str());
        let Ok(_pathBuf) = self.dir.canonicalize(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to canonicalize path")});
        };
        let Some(_str) = _pathBuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Path contains non utf-8 characters")});
        };
        result.get().set_path_buf(_str.into());
        Promise::ok(())
    }
    fn copy(&mut self, params: dir::CopyParams, mut result: dir::CopyResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let from = pry!(pry!(params_reader.get_path_from()).to_str());
        let to = pry!(pry!(params_reader.get_path_to()).to_str());
        let dir_to_cap = pry!(params_reader.get_dir_to());
        let Some(dir_impl) = DIR_SET.with_borrow(|set| set.get_local_server_of_resolved(&dir_to_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Dir not from the same machine")});
        };
        let dir_to = &dir_impl.borrow().dir;
        let Ok(bytes) = self.dir.copy(from, dir_to, to) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to copy file contents")});
        };
        result.get().set_result(bytes);
        Promise::ok(())
    }
    fn hard_link(&mut self, params: dir::HardLinkParams, _: dir::HardLinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let src = pry!(pry!(params_reader.get_src_path()).to_str());
        let dst = pry!(pry!(params_reader.get_dst_path()).to_str());
        let dst_dir_cap = pry!(params_reader.get_dst_dir());
        let Some(dir_impl) = DIR_SET.with_borrow(|set| set.get_local_server_of_resolved(&dst_dir_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Dir not from the same machine")});
        };
        let dst_dir = &dir_impl.borrow().dir;
        let Ok(()) = self.dir.hard_link(src, dst_dir, dst) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create hard link")});
        };
        Promise::ok(())
    }
    fn metadata(&mut self, params: dir::MetadataParams, mut result: dir::MetadataResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(_metadata) = self.dir.metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get file metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn dir_metadata(&mut self, _: dir::DirMetadataParams, mut result: dir::DirMetadataResults) -> Promise<(), Error> {
        let Ok(_metadata) = self.dir.dir_metadata() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get dir metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn entries(&mut self, _: dir::EntriesParams, mut result: dir::EntriesResults) -> Promise<(), Error> {
        let Ok(mut _iter) = self.dir.entries() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get dir entries")});
        };
        result.get().set_iter(capnp_rpc::new_client(ReadDirImpl{iter:_iter}));
        Promise::ok(())
    }
    fn read_dir(&mut self, params: dir::ReadDirParams, mut result: dir::ReadDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(mut _iter) = self.dir.read_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read dir")});
        };
        result.get().set_iter(capnp_rpc::new_client(ReadDirImpl{iter:_iter}));
        Promise::ok(())
    }
    fn read(&mut self, params: dir::ReadParams, mut result: dir::ReadResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(vec) = self.dir.read(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read file")});
        };
        result.get().set_result(vec.as_slice());
        Promise::ok(())
    }
    fn read_link(&mut self, params: dir::ReadLinkParams, mut result: dir::ReadLinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(_pathbuf) = self.dir.read_link(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read link")});
        };
        let Some(_str) = _pathbuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read link")});
        };
        result.get().set_result(_str.into());
        Promise::ok(())
    }
    fn read_to_string(&mut self, params: dir::ReadToStringParams, mut result: dir::ReadToStringResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(string) = self.dir.read_to_string(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read file to string")});
        };
        result.get().set_result(string.as_str().into());
        Promise::ok(())
    }
    fn remove_dir(&mut self, params: dir::RemoveDirParams, _: dir::RemoveDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(()) = self.dir.remove_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove dir")});
        };
        Promise::ok(())
    }
    fn remove_dir_all(&mut self, params: dir::RemoveDirAllParams, _: dir::RemoveDirAllResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(()) = self.dir.remove_dir_all(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove dir(all)")});
        };
        Promise::ok(())
    }
    fn remove_open_dir(&mut self, _: dir::RemoveOpenDirParams, _: dir::RemoveOpenDirResults) -> Promise<(), Error> {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create an owned dir")});
        };
        let Ok(()) = this.remove_open_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove open dir")});
        };
        Promise::ok(())
    }
    fn remove_open_dir_all(&mut self, _: dir::RemoveOpenDirAllParams, _: dir::RemoveOpenDirAllResults) -> Promise<(), Error> {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create an owned dir")});
        };
        let Ok(()) = this.remove_open_dir_all() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove open dir(all)")});
        };
        Promise::ok(())
    }
    fn remove_file(&mut self, params: dir::RemoveFileParams, _: dir::RemoveFileResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(()) = self.dir.remove_file(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove file")});
        };
        Promise::ok(())
    }
    fn rename(&mut self, params: dir::RenameParams, _: dir::RenameResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let from = pry!(pry!(params_reader.get_from()).to_str());
        let to = pry!(pry!(params_reader.get_to()).to_str());
        let this = &self.dir;
        let Ok(()) = self.dir.rename(from, this, to) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to rename file")});
        };
        Promise::ok(())
    }
    fn set_readonly(&mut self, params: dir::SetReadonlyParams, _: dir::SetReadonlyResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let readonly = params_reader.get_readonly();
        //capnp_let!({path, readonly} = params_reader);
        let Ok(_meta) = self.dir.metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get underlying file's metadata")});
        };
        let mut permissions = _meta.permissions();
        permissions.set_readonly(readonly);
        let Ok(()) = self.dir.set_permissions(path, permissions) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to change permissions of the underlying file")});
        };
        Promise::ok(())
    }
    fn symlink_metadata(&mut self, params: dir::SymlinkMetadataParams, mut result: dir::SymlinkMetadataResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(_metadata) = self.dir.symlink_metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get symlink metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn write(&mut self, params: dir::WriteParams, _: dir::WriteResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let contents = pry!(params_reader.get_contents());
        let Ok(()) = self.dir.write(path, contents) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to write to file")});
        };
        Promise::ok(())
    }
    fn symlink(&mut self, params: dir::SymlinkParams, _: dir::SymlinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let original = pry!(pry!(params_reader.get_original()).to_str());
        let link = pry!(pry!(params_reader.get_link()).to_str());
        #[cfg(target_os = "windows")]
        let Ok(()) = self.dir.symlink_dir(original, link) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create symlink")});
        };
        #[cfg(not(target_os = "windows"))]
        let Ok(()) = self.dir.symlink(original, link) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create symlink")});
        };
        Promise::ok(())
    }
    fn exists(&mut self, params: dir::ExistsParams, mut result: dir::ExistsResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let _result = self.dir.exists(path);
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn try_exists(&mut self, params: dir::TryExistsParams, mut result: dir::TryExistsResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let Ok(_result) = self.dir.try_exists(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to check if entity exists")});
        };
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_file(&mut self, params: dir::IsFileParams, mut result: dir::IsFileResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let _result = self.dir.is_file(path);
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_dir(&mut self, params: dir::IsDirParams, mut result: dir::IsDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(pry!(params_reader.get_path()).to_str());
        let _result = self.dir.is_dir(path);
        result.get().set_result(_result);
        Promise::ok(())
    }


    fn temp_dir_new_in(&mut self, _: dir::TempDirNewInParams, mut result: dir::TempDirNewInResults) -> Promise<(), Error> {
        let Ok(temp_dir) = cap_tempfile::TempDir::new_in(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp dir")});
        };
        result.get().set_temp_dir(capnp_rpc::new_client(TempDirImpl{temp_dir: Some(temp_dir)}));
        Promise::ok(())
    }/* 
    fn temp_file_new(&mut self,  params: dir::TempFileNewParams, mut result: dir::TempFileNewResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let dir_cap = pry!(params_reader.get_dir());
        let Some(underlying_dir) = DIR_SET.with_borrow(|set| set.get_local_server_of_resolved(&dir_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Dir not from the same machine")});
        };
        let dir = underlying_dir.borrow().dir.try_clone().unwrap();
        let Ok(temp_file) = cap_tempfile::TempFile::new(&dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp file")});
        };
        result.get().set_temp_file(capnp_rpc::new_client(TempFileImpl{temp_file: Some(temp_file)}));
        Promise::ok(())
    }*/
    fn temp_file_new_anonymous(&mut self, _: dir::TempFileNewAnonymousParams, mut result: dir::TempFileNewAnonymousResults) -> Promise<(), Error> {
        let Ok(file) = cap_tempfile::TempFile::new_anonymous(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create anonymous temp file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: file}));
        Promise::ok(())
    }
}

pub struct ReadDirImpl {
    iter: ReadDir
}

impl read_dir::Server for ReadDirImpl {
    fn next(&mut self, _: read_dir::NextParams, mut result: read_dir::NextResults) -> Promise<(), Error> {
        let Some(_result) = self.iter.next() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Final entry reached")});
        };
        let Ok(_entry) = _result else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Encountered an error while getting dir entry")});
        };
        result.get().set_entry(capnp_rpc::new_client(DirEntryImpl{entry: _entry}));
        Promise::ok(())
    }
}

pub struct DirEntryImpl {
    entry: DirEntry
}

impl dir_entry::Server for DirEntryImpl {
    fn open(&mut self, _: dir_entry::OpenParams, mut result: dir_entry::OpenResults) -> Promise<(), Error> {
        let Ok(_file) = self.entry.open() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file for reading")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    
    fn open_with(&mut self, params: dir_entry::OpenWithParams, mut result: dir_entry::OpenWithResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        capnp_let!({open_options : {read, write, append, truncate, create, create_new}} = params_reader);
        let mut options = OpenOptions::new();
        options.read(read).write(write).append(append).truncate(truncate).create(create).create_new(create_new);
        let Ok(_file) = self.entry.open_with(&options) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file for reading(With custom options)")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn open_dir(&mut self, _: dir_entry::OpenDirParams, mut result: dir_entry::OpenDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.entry.open_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open the entry as a dir")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn remove_file(&mut self, _: dir_entry::RemoveFileParams, _: dir_entry::RemoveFileResults) -> Promise<(), Error> {
        let Ok(()) = self.entry.remove_file() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove file")});
        };
        Promise::ok(())
    }
    fn remove_dir(&mut self, _: dir_entry::RemoveDirParams, _: dir_entry::RemoveDirResults) -> Promise<(), Error> {
        let Ok(()) = self.entry.remove_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove dir")});
        };
        Promise::ok(())
    }
    fn metadata(&mut self, _: dir_entry::MetadataParams, mut result: dir_entry::MetadataResults) -> Promise<(), Error> {
        let Ok(_metadata) = self.entry.metadata() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get file metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn file_type(&mut self, _: dir_entry::FileTypeParams, mut result: dir_entry::FileTypeResults) -> Promise<(), Error> {
        let Ok(_file_type) = self.entry.file_type() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get file type")});
        };
        let _type: FileType = if _file_type.is_dir() {FileType::Dir} else if _file_type.is_file() {FileType::File} else if _file_type.is_symlink() {FileType::Symlink} 
        else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Unknown file type")})
        };
        result.get().set_type(_type);
        Promise::ok(())
    }
    fn file_name(&mut self, _: dir_entry::FileNameParams, mut result: dir_entry::FileNameResults) -> Promise<(), Error> {
        let _name = self.entry.file_name();
        let Some(_name) = _name.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("File name not valid utf-8")});
        };
        result.get().set_result(_name.into());
        Promise::ok(())
    }
}

pub struct FileImpl {
    file: File
}

impl file::Server for FileImpl {
    fn sync_all(&mut self, _: file::SyncAllParams, _: file::SyncAllResults) -> Promise<(), Error> {
        let Ok(()) = self.file.sync_all() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to sync os-internal metadata to disk")});
        };
        Promise::ok(())
    }
    fn sync_data(&mut self, _: file::SyncDataParams, _: file::SyncDataResults) -> Promise<(), Error> {
        let Ok(()) = self.file.sync_data() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to sync os-internal metadata to sync data")});
        };
        Promise::ok(())
    }
    fn set_len(&mut self, params: file::SetLenParams, _: file::SetLenResults) -> Promise<(), Error> {
        let size_reader = pry!(params.get());
        let size = size_reader.get_size();
        let Ok(()) = self.file.set_len(size) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to update the size of file")});
        };
        Promise::ok(())
    }
    fn metadata(&mut self, _: file::MetadataParams, mut result: file::MetadataResults) -> Promise<(), Error> {
        let Ok(_metadata) = self.file.metadata() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Get file metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn try_clone(&mut self, _: file::TryCloneParams, mut result: file::TryCloneResults) -> Promise<(), Error> {
        let Ok(_file) = self.file.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to sync os-internal metadata to sync data")});
        };
        result.get().set_cloned(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn set_readonly(&mut self, params: file::SetReadonlyParams,  _: file::SetReadonlyResults) -> Promise<(), Error> {
        let readonly_reader = pry!(params.get());
        let readonly = readonly_reader.get_readonly();
        let Ok(_meta) = self.file.metadata() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get file's metadata")});
        };
        let mut permissions = _meta.permissions();
        permissions.set_readonly(readonly);
        let Ok(()) = self.file.set_permissions(permissions) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to change permissions of the file")});
        };
        Promise::ok(())
    }
    
    fn open(&mut self, _: file::OpenParams, mut result: file::OpenResults) -> Promise<(), Error> {
        let Ok(mut this) = self.file.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Get owned file")});
        };
        let _stream = ByteStreamImpl::new(move |bytes| {
            let Ok(()) = this.write_all(bytes) else {
                return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to write to file")});
            };
            Promise::ok(())
        });
        result.get().set_stream(capnp_rpc::new_client(_stream));
        Promise::ok(())
    }
}

pub struct MetadataImpl {
    metadata: Metadata
}

impl metadata::Server for MetadataImpl {
    fn file_type(&mut self, _: metadata::FileTypeParams, mut result: metadata::FileTypeResults) -> Promise<(), Error> {
        let _file_type = self.metadata.file_type();
        let _type: FileType = if _file_type.is_dir() {FileType::Dir} else if _file_type.is_file() {FileType::File} else if _file_type.is_symlink() {FileType::Symlink} 
        else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Unknown file type")})
        };
        result.get().set_file_type(_type);
        Promise::ok(())
    }
    fn is_dir(&mut self, _: metadata::IsDirParams, mut result: metadata::IsDirResults) -> Promise<(), Error> {
        let _result = self.metadata.is_dir();
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_file(&mut self, _: metadata::IsFileParams, mut result: metadata::IsFileResults) -> Promise<(), Error> {
        let _result = self.metadata.is_file();
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_symlink(&mut self, _: metadata::IsSymlinkParams, mut result: metadata::IsSymlinkResults) -> Promise<(), Error> {
        let _result = self.metadata.is_symlink();
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn len(&mut self, _: metadata::LenParams, mut result: metadata::LenResults) -> Promise<(), Error> {
        let _result = self.metadata.len();
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn permissions(&mut self, _: metadata::PermissionsParams, mut result: metadata::PermissionsResults) -> Promise<(), Error> {
        let _permissions = self.metadata.permissions();
        result.get().set_permissions(capnp_rpc::new_client(PermissionsImpl{permissions: _permissions}));
        Promise::ok(())
    }
    fn modified(&mut self, _: metadata::ModifiedParams, mut result: metadata::ModifiedResults) -> Promise<(), Error> {
        let Ok(_time) = self.metadata.modified() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to access modified field of the metadata")});
        };
        result.get().set_time(capnp_rpc::new_client(SystemTimeImpl{system_time: _time}));
        Promise::ok(())
    }
    fn accessed(&mut self, _: metadata::AccessedParams, mut result: metadata::AccessedResults) -> Promise<(), Error> {
        let Ok(_time) = self.metadata.accessed() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to access accessed field of the metadata")});
        };
        result.get().set_time(capnp_rpc::new_client(SystemTimeImpl{system_time: _time}));
        Promise::ok(())
    }
    fn created(&mut self, _: metadata::CreatedParams, mut result: metadata::CreatedResults) -> Promise<(), Error> {
        let Ok(_time) = self.metadata.created() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to access created field of the metadata")});
        };
        result.get().set_time(capnp_rpc::new_client(SystemTimeImpl{system_time: _time}));
        Promise::ok(())
    }
}

pub struct PermissionsImpl {
    permissions: Permissions
}

impl permissions::Server for PermissionsImpl {
    fn readonly(&mut self, _: permissions::ReadonlyParams, mut result: permissions::ReadonlyResults) -> Promise<(), Error> {
        let _result = self.permissions.readonly();
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn set_readonly(&mut self, params: permissions::SetReadonlyParams, _: permissions::SetReadonlyResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let readonly = params_reader.get_readonly();
        self.permissions.set_readonly(readonly);
        Promise::ok(())
    }
}

pub struct TempDirImpl {
    temp_dir: Option<TempDir>
}

impl temp_dir::Server for TempDirImpl {
    fn close(&mut self, _: temp_dir::CloseParams, _: temp_dir::CloseResults) -> Promise<(), Error> {
        let Some(dir) = self.temp_dir.take() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Temp dir already closed")});
        };
        let Ok(()) = dir.close() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to close temp dir")});
        };
        Promise::ok(())
    }
    fn get_as_dir(&mut self, _: temp_dir::GetAsDirParams, mut result: temp_dir::GetAsDirResults) -> Promise<(), Error> {
        let Some(dir) = self.temp_dir.take() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Temp dir already closed")});
        };
        let Ok(cloned) = dir.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get an owned version of the underlying dir")});
        };
        self.temp_dir = Some(dir);
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: cloned})));
        Promise::ok(())
    }
}
pub struct TempFileImpl<'a> {
    temp_file: Option<TempFile<'a>>
}

impl temp_file::Server for TempFileImpl<'_> {
    fn as_file(&mut self, _: temp_file::AsFileParams, mut result: temp_file::AsFileResults) -> Promise<(), Error> {
        let Some(_file) = self.temp_file.take() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Temp file already removed")});
        };
        let Ok(_cloned) = _file.as_file().try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get an owned version of the underlying file")});
        };
        self.temp_file = Some(_file);
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _cloned}));
        Promise::ok(())
    }
    fn replace(&mut self, params: temp_file::ReplaceParams, _: temp_file::ReplaceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let dest = pry!(pry!(params_reader.get_dest()).to_str());
        let Some(temp_file) = self.temp_file.take() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Temp file already removed")});
        };
        let Ok(()) = temp_file.replace(dest) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to write file to the target location")});
        };
        Promise::ok(())
    }
}

pub struct SystemTimeImpl {
    system_time: SystemTime
}

impl system_time::Server for SystemTimeImpl {
    fn duration_since(&mut self, params: system_time::DurationSinceParams, mut result: system_time::DurationSinceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let earlier = pry!(params_reader.get_earlier());
        let Ok(earlier_duration_since_unix_epoch) = tokio::task::block_in_place(move ||tokio::runtime::Handle::current().block_on(earlier.get_duration_since_unix_epoch_request().send().promise)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to convert to unix time")});
        };
        let reader = pry!(earlier_duration_since_unix_epoch.get());
        capnp_let!({duration : {secs, nanos}} = reader);
        //Add duration since unix epoch to unix epoch to reconstruct a system time
        let earlier = cap_std::time::SystemTime::from_std(std::time::UNIX_EPOCH + Duration::new(secs, nanos));
        let Ok(_duration_since) = self.system_time.duration_since(earlier) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("System time earlier than self")});
        };
        let mut response = result.get().init_duration();
        response.set_secs(_duration_since.as_secs());
        response.set_nanos(_duration_since.subsec_nanos());
        Promise::ok(())
    }

    fn checked_add(&mut self, params: system_time::CheckedAddParams, mut result: system_time::CheckedAddResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let duration_reader = pry!(params_reader.get_duration());
        let secs = duration_reader.get_secs();
        let nanos = duration_reader.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_time) = self.system_time.checked_add(duration) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to add duration to system time")});
        };
        result.get().set_result(capnp_rpc::new_client(SystemTimeImpl{system_time: _time}));
        Promise::ok(())
    }

    fn checked_sub(&mut self, params: system_time::CheckedSubParams, mut result: system_time::CheckedSubResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let duration_reader = pry!(params_reader.get_duration());
        let secs = duration_reader.get_secs();
        let nanos = duration_reader.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_time) = self.system_time.checked_sub(duration) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to subtract duration from system time")});
        };
        result.get().set_result(capnp_rpc::new_client(SystemTimeImpl{system_time: _time}));
        Promise::ok(())
    }
    
    fn get_duration_since_unix_epoch(&mut self, _: system_time::GetDurationSinceUnixEpochParams, mut result: system_time::GetDurationSinceUnixEpochResults) -> Promise<(), Error> {
        let Ok(_duration) = self.system_time.into_std().duration_since(std::time::UNIX_EPOCH) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get duration since unix epoch")});
        };
        let mut response = result.get().init_duration();
        response.set_secs(_duration.as_secs());
        response.set_nanos(_duration.subsec_nanos());
        Promise::ok(())
    }
}

pub struct InstantImpl {
    instant: Instant
}

impl instant::Server for InstantImpl {
    fn duration_since(&mut self, params: instant::DurationSinceParams, mut result: instant::DurationSinceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let instant_cap = pry!(params_reader.get_earlier());
        let Some(instant_impl) = INSTANT_SET.with_borrow(|set| set.get_local_server_of_resolved(&instant_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Earlier instant not from the same machine")});
        };
        let earlier = instant_impl.borrow().instant.clone();
        let dur = self.instant.duration_since(earlier);
        let mut response = result.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Promise::ok(())
    }
    fn checked_duration_since(&mut self, params: instant::CheckedDurationSinceParams, mut result: instant::CheckedDurationSinceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let instant_cap = pry!(params_reader.get_earlier());
        let Some(instant_impl) = INSTANT_SET.with_borrow(|set| set.get_local_server_of_resolved(&instant_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Earlier instant not from the same machine")});
        };
        let earlier = instant_impl.borrow().instant.clone();
        let Some(dur) = self.instant.checked_duration_since(earlier) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Earlier instant not from the same machine")});
        };
        let mut response = result.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Promise::ok(())
    }
    fn saturating_duration_since(&mut self, params: instant::SaturatingDurationSinceParams, mut result: instant::SaturatingDurationSinceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let instant_cap = pry!(params_reader.get_earlier());
        let Some(instant_impl) = INSTANT_SET.with_borrow(|set| set.get_local_server_of_resolved(&instant_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Earlier instant not from the same machine")});
        };
        let earlier = instant_impl.borrow().instant.clone();
        let dur = self.instant.saturating_duration_since(earlier);
        let mut response = result.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Promise::ok(())
    }
    fn checked_add(&mut self, params: instant::CheckedAddParams, mut result: instant::CheckedAddResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let duration_reader = pry!(params_reader.get_duration());
        let secs = duration_reader.get_secs();
        let nanos = duration_reader.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_instant) = self.instant.checked_add(duration) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to add duration to instant")});
        };
        result.get().set_instant(INSTANT_SET.with_borrow_mut(|set| set.new_client(InstantImpl{instant: _instant})));
        Promise::ok(())
    }
    fn checked_sub(&mut self, params: instant::CheckedSubParams, mut result: instant::CheckedSubResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let duration_reader = pry!(params_reader.get_duration());
        let secs = duration_reader.get_secs();
        let nanos = duration_reader.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_instant) = self.instant.checked_sub(duration) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to subtract duration from instant")});
        };
        result.get().set_instant(INSTANT_SET.with_borrow_mut(|set| set.new_client(InstantImpl{instant: _instant})));
        Promise::ok(())
    }
}

pub struct MonotonicClockImpl {
    monotonic_clock: MonotonicClock
}

impl monotonic_clock::Server for MonotonicClockImpl {
    fn now(&mut self, _: monotonic_clock::NowParams, mut result: monotonic_clock::NowResults) -> Promise<(), Error> {
        let _instant = self.monotonic_clock.now();
        result.get().set_instant(INSTANT_SET.with_borrow_mut(|set| set.new_client(InstantImpl{instant: _instant})));
        Promise::ok(())
    }
    fn elapsed(&mut self, params: monotonic_clock::ElapsedParams, mut result: monotonic_clock::ElapsedResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let instant_cap = pry!(params_reader.get_instant());
        let Some(instant_impl) = INSTANT_SET.with_borrow(|set| set.get_local_server_of_resolved(&instant_cap)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Earlier instant not from the same machine")});
        };
        let instant = instant_impl.borrow().instant.clone();
        let dur = self.monotonic_clock.elapsed(instant);
        let mut response = result.get().init_duration();
        response.set_secs(dur.as_secs());
        response.set_nanos(dur.subsec_nanos());
        Promise::ok(())
    }
}

pub struct SystemClockImpl {
    system_clock: SystemClock
}

impl system_clock::Server for SystemClockImpl {
    fn now(&mut self, _: system_clock::NowParams, mut result: system_clock::NowResults) -> Promise<(), Error> {
        let _system_time = self.system_clock.now();
        result.get().set_time(capnp_rpc::new_client(SystemTimeImpl{system_time: _system_time}));
        Promise::ok(())
    }
    fn elapsed(&mut self, params: system_clock::ElapsedParams, mut result: system_clock::ElapsedResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        capnp_let!({duration_since_unix_epoch : {secs, nanos}} = params_reader);
        //Add duration since unix epoch to unix epoch to reconstruct a system time
        let earlier = cap_std::time::SystemTime::from_std(std::time::UNIX_EPOCH + Duration::new(secs, nanos));
        let Ok(_elapsed) = self.system_clock.elapsed(earlier) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get amount of time elapsed")});
        };
        let mut response = result.get().init_result();
        response.set_secs(_elapsed.as_secs());
        response.set_nanos(_elapsed.subsec_nanos());
        Promise::ok(())
    }
}

pub struct ProjectDirsImpl {
    project_dirs: ProjectDirs
}

impl project_dirs::Server for ProjectDirsImpl {
    fn cache_dir(&mut self, _: project_dirs::CacheDirParams, mut result: project_dirs::CacheDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.cache_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve cache directory")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn config_dir( &mut self, _: project_dirs::ConfigDirParams, mut result: project_dirs::ConfigDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.config_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve config directory")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn data_dir(&mut self, _: project_dirs::DataDirParams, mut result: project_dirs::DataDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.data_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve data directory")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn data_local_dir(&mut self, _: project_dirs::DataLocalDirParams, mut result: project_dirs::DataLocalDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.data_local_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve local data directory")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
    fn runtime_dir(&mut self, _: project_dirs::RuntimeDirParams, mut result: project_dirs::RuntimeDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.runtime_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve runtime directory")});
        };
        result.get().set_dir(DIR_SET.with_borrow_mut(|set| set.new_client(DirImpl{dir: _dir})));
        Promise::ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use std::{path::Path};

    use cap_std::{fs::{Dir, DirBuilder, File, Metadata, ReadDir, Permissions, OpenOptions, DirEntry}, time::{MonotonicClock, SystemClock, SystemTime, Duration, Instant}};
    use cap_tempfile::{TempFile, TempDir};
    use cap_directories::{self, UserDirs, ProjectDirs};
    use capnp_rpc::pry;
    use capnp_macros::capnp_let;
    use tokio::io::{AsyncRead, AsyncReadExt};
    use crate::{cap_std_capnp::{ambient_authority, dir, dir_entry, duration, file, FileType, instant, metadata, monotonic_clock, open_options, permissions, project_dirs, read_dir, system_clock, system_time, temp_dir, temp_file, user_dirs}, spawn::unix_process::UnixProcessServiceSpawnImpl, byte_stream::ByteStreamImpl, cap_std_capnproto::AmbientAuthorityImpl};
    

    #[test]
    fn create_dir_all_canonicalize_test() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});
        
        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        open_ambient_request.get().set_path(path.to_str().unwrap().into());
        let dir = futures::executor::block_on(open_ambient_request.send().promise)?.get()?.get_result()?;

        let mut create_dir_all_request = dir.create_dir_all_request();
        create_dir_all_request.get().set_path("test_dir/testing_recursively_creating_dirs".into());
        futures::executor::block_on(create_dir_all_request.send().promise)?;

        let mut canonicalize_request = dir.canonicalize_request();
        canonicalize_request.get().set_path("test_dir/testing_recursively_creating_dirs".into());
        let result = futures::executor::block_on(canonicalize_request.send().promise)?;
        let p = result.get()?.get_path_buf()?.to_str()?;
        println!("path = {p}");
        return Ok(())
    }
    
    #[test]
    fn test_create_write_getmetadata() -> eyre::Result<()> {
        //use ambient authority to open a dir, create a file(Or open it in write mode if it already exists), open a bytestream, use the bytestream to write some bytes, get file metadata
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});

        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        open_ambient_request.get().set_path(path.to_str().unwrap().into());
        let dir = futures::executor::block_on(open_ambient_request.send().promise)?.get()?.get_result()?;

        let mut create_request = dir.create_request();
        create_request.get().set_path("capnp_test.txt".into());
        let file = futures::executor::block_on(create_request.send().promise)?.get()?.get_file()?;

        let mut open_bytestream_request = file.open_request();
        let stream = futures::executor::block_on(open_bytestream_request.send().promise)?.get()?.get_stream()?;

        let mut write_request = stream.write_request();
        write_request.get().set_bytes(b"Writing some bytes test ");
        let _res = futures::executor::block_on(write_request.send().promise)?;

        let mut file_metadata_request = dir.metadata_request();
        file_metadata_request.get().set_path("capnp_test.txt".into());
        let metadata = futures::executor::block_on(file_metadata_request.send().promise)?.get()?.get_metadata()?;
        println!("File metadata:");
        test_metadata(metadata)?;

        let mut dir_metadata_request = dir.dir_metadata_request();
        let metadata = futures::executor::block_on(dir_metadata_request.send().promise)?.get()?.get_metadata()?;
        println!("Dir metadata:");
        test_metadata(metadata)?;

        return Ok(())
    }
    
    #[test]
    fn test_home_dir() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});

        let home_dir_request = ambient_authority.user_dirs_home_dir_request();
        let home_dir = futures::executor::block_on(home_dir_request.send().promise)?.get()?.get_dir()?;
        return Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_user_dirs() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});

        let audio_dir_request = ambient_authority.user_dirs_audio_dir_request();
        let audio_dir = futures::executor::block_on(audio_dir_request.send().promise)?.get()?.get_dir()?;

        let desktop_dir_request = ambient_authority.user_dirs_desktop_dir_request();
        let desktop_dir = futures::executor::block_on(desktop_dir_request.send().promise)?.get()?.get_dir()?;

        let document_dir_request = ambient_authority.user_dirs_document_dir_request();
        let document_dir = futures::executor::block_on(document_dir_request.send().promise)?.get()?.get_dir()?;

        let download_dir_request = ambient_authority.user_dirs_download_dir_request();
        let download_dir = futures::executor::block_on(download_dir_request.send().promise)?.get()?.get_dir()?;

        #[cfg(not(target_os = "windows"))]
        let font_dir_request = ambient_authority.user_dirs_font_dir_request();
        #[cfg(not(target_os = "windows"))]
        let font_dir = futures::executor::block_on(font_dir_request.send().promise)?.get()?.get_dir()?;

        let picture_dir_request = ambient_authority.user_dirs_picture_dir_request();
        let picture_dir = futures::executor::block_on(picture_dir_request.send().promise)?.get()?.get_dir()?;

        let public_dir_request = ambient_authority.user_dirs_public_dir_request();
        let public_dir = futures::executor::block_on(public_dir_request.send().promise)?.get()?.get_dir()?;

        let template_dir_request = ambient_authority.user_dirs_template_dir_request();
        let template_dir = futures::executor::block_on(template_dir_request.send().promise)?.get()?.get_dir()?;

        let video_dir_request = ambient_authority.user_dirs_video_dir_request();
        let video_dir = futures::executor::block_on(video_dir_request.send().promise)?.get()?.get_dir()?;

        return Ok(())
    }

    #[test]
    fn test_project_dirs() -> eyre::Result<()> {
        //TODO maybe create some form of generic "dir" test
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});

        let mut project_dirs_from_request = ambient_authority.project_dirs_from_request();
        let mut project_dirs_builder = project_dirs_from_request.get();
        project_dirs_builder.set_qualifier("".into());
        project_dirs_builder.set_organization("Fundament software".into());
        project_dirs_builder.set_application("Keystone".into());
        let project_dirs = futures::executor::block_on(project_dirs_from_request.send().promise)?.get()?.get_project_dirs()?;
        
        let cache_dir_request = project_dirs.cache_dir_request();
        let cache_dir = futures::executor::block_on(cache_dir_request.send().promise)?.get()?.get_dir()?;

        let config_dir_request = project_dirs.config_dir_request();
        let config_dir = futures::executor::block_on(config_dir_request.send().promise)?.get()?.get_dir()?;

        let data_dir_request = project_dirs.data_dir_request();
        let data_dir = futures::executor::block_on(data_dir_request.send().promise)?.get()?.get_dir()?;

        let data_local_dir_request = project_dirs.data_local_dir_request();
        let data_local_dir = futures::executor::block_on(data_local_dir_request.send().promise)?.get()?.get_dir()?;

        //TODO Runtime directory seems to not exist on 
        #[cfg(not(target_os = "windows"))]
        let runtime_dir_request = project_dirs.runtime_dir_request();
        #[cfg(not(target_os = "windows"))]
        let runtime_dir = futures::executor::block_on(runtime_dir_request.send().promise)?.get()?.get_dir()?;

        return Ok(())
    }

    #[test]
    fn test_system_clock() -> eyre::Result<()> {
        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});

        let system_clock_request = ambient_authority.system_clock_new_request();
        let system_clock = futures::executor::block_on(system_clock_request.send().promise)?.get()?.get_clock()?;

        let now_request = system_clock.now_request();
        let now = futures::executor::block_on(now_request.send().promise)?.get()?.get_time()?;

        let duration_since_unix_epoch_request = now.get_duration_since_unix_epoch_request();
        let result = futures::executor::block_on(duration_since_unix_epoch_request.send().promise)?;
        let duration_since_unix_epoch = result.get()?.get_duration()?;
        let secs = duration_since_unix_epoch.get_secs();
        let nanos = duration_since_unix_epoch.get_nanos();
        print!("\nDuration since unix epoch to now: secs:{secs} nanos:{nanos}");

        print!(" waiting 5 seconds ");
        std::thread::sleep(std::time::Duration::from_secs(5));

        let mut elapsed_request = system_clock.elapsed_request();
        let mut dur_param = elapsed_request.get().init_duration_since_unix_epoch();
        dur_param.set_secs(secs);
        dur_param.set_nanos(nanos);
        let result = futures::executor::block_on(elapsed_request.send().promise)?;
        let elapsed = result.get()?.get_result()?;
        let secs = elapsed.get_secs();
        let nanos = elapsed.get_nanos();
        print!("elapsed since last: secs:{secs} nanos{nanos}\n");

        return Ok(())
    }

    #[test]
    fn test_read_dir_iterator() -> eyre::Result<()> {
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

        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});
        
        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        path.push("capnp_test_dir");
        open_ambient_request.get().set_path(path.to_str().unwrap().into());
        let dir = futures::executor::block_on(open_ambient_request.send().promise)?.get()?.get_result()?;

        let entries_request = dir.entries_request();
        let iter = futures::executor::block_on(entries_request.send().promise)?.get()?.get_iter()?;
        loop {
            match futures::executor::block_on(iter.next_request().send().promise){
                Ok(result) => {
                    println!("New entry:");
                    let entry = result.get()?.get_entry()?;
                    dir_entry_test(entry)?;
                },
                Err(result) => {
                    println!("Final entry reached");
                    break;
                }
            }
        }
        return Ok(())
    }
    
    fn dir_entry_test(entry: dir_entry::Client) -> eyre::Result<()> {
        println!("\nDir entry test:");
        match futures::executor::block_on(entry.open_request().send().promise) {
            Ok(result) => {
                println!("Is file");
                let file = result.get()?.get_file()?;
            },
            Err(result) => println!("Isn't file")
        }

        match futures::executor::block_on(entry.open_dir_request().send().promise) {
            Ok(result) => {
                println!("Is dir");
                let dir = result.get()?.get_dir()?;
            },
            Err(result) => println!("Isn't dir")
        }

        let metadata = futures::executor::block_on(entry.metadata_request().send().promise)?.get()?.get_metadata()?;
        test_metadata(metadata)?;

        let file_type = futures::executor::block_on(entry.file_type_request().send().promise)?.get()?.get_type()?;
        println!("File type = {:?}", file_type);

        let result = futures::executor::block_on(entry.file_name_request().send().promise)?;
        let name = result.get()?.get_result()?.to_str()?;
        println!("File/dir name: {name}");
        return Ok(())
    }

    pub fn test_metadata(metadata: metadata::Client) -> eyre::Result<()> {
        println!("\nMetadata test:");
        let is_dir_request = metadata.is_dir_request();
        let result = futures::executor::block_on(is_dir_request.send().promise)?.get()?.get_result();
        println!("Is dir: {result}");

        let is_file_request = metadata.is_file_request();
        let result = futures::executor::block_on(is_file_request.send().promise)?.get()?.get_result();
        println!("Is file: {result}");

        let is_symlink_request = metadata.is_symlink_request();
        let result = futures::executor::block_on(is_symlink_request.send().promise)?.get()?.get_result();
        println!("Is symlink: {result}");

        let len_request = metadata.len_request();
        let result = futures::executor::block_on(len_request.send().promise)?.get()?.get_result();
        println!("Len: {result}");

        let file_type_request = metadata.file_type_request();
        let file_type =futures::executor::block_on(file_type_request.send().promise)?.get()?.get_file_type()?;
        println!("File type = {:?}", file_type);

        let permissions_request = metadata.permissions_request();
        let permissions = futures::executor::block_on(permissions_request.send().promise)?.get()?.get_permissions()?;
        test_permissions(permissions)?;

        let mut modified_request = metadata.modified_request();
        let modified_time = futures::executor::block_on(modified_request.send().promise)?.get()?.get_time()?;
        println!("Modified time:");
        test_system_time(modified_time)?;

        let mut accessed_request = metadata.accessed_request();
        let accessed_time = futures::executor::block_on(accessed_request.send().promise)?.get()?.get_time()?;
        println!("Accessed time:");
        test_system_time(accessed_time)?;

        let mut created_request = metadata.created_request();
        let created_time = futures::executor::block_on(created_request.send().promise)?.get()?.get_time()?;
        println!("Created time:");
        test_system_time(created_time)?;

        return Ok(())
    }

    fn test_permissions(permissions: permissions::Client) -> eyre::Result<()> {
        println!("\nPermissions test:");
        let readonly_request = permissions.readonly_request();
        let result = futures::executor::block_on(readonly_request.send().promise)?.get()?.get_result();
        println!("Is readonly: {result}");

        //TODO test setting readonly
        return Ok(())
    }

    fn test_system_time(time: system_time::Client) -> eyre::Result<()> {
        println!("\nSystem time test:");
        //TODO test other stuff


        let get_duration_since_unix_epoch_request = time.get_duration_since_unix_epoch_request();
        let result = futures::executor::block_on(get_duration_since_unix_epoch_request.send().promise)?;
        let duration = result.get()?.get_duration()?;
        let secs = duration.get_secs();
        let nanos = duration.get_nanos();
        //capnp_let!({secs, nanos} = duration);
        println!("Duration since unix epoch: secs:{secs} nanos:{nanos}");
        return Ok(())
    }

    #[test]
    fn test_open_read() -> eyre::Result<()> {
        //use ambient authority to open directory, read contents of a file as bytes and print them out
        use std::io::{BufWriter, Write};

        let mut path = std::env::temp_dir();
        path.push("capnp_test.txt");
        let _f = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(_f);
        writer.write_all(b"Just a test file ")?;
        writer.flush()?;

        let ambient_authority: ambient_authority::Client = capnp_rpc::new_client(AmbientAuthorityImpl{});
        
        let mut open_ambient_request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        open_ambient_request.get().set_path(path.to_str().unwrap().into());
        let dir = futures::executor::block_on(open_ambient_request.send().promise)?.get()?.get_result()?;

        let mut read_request = dir.read_request();
        read_request.get().set_path("capnp_test.txt".into());
        let res = futures::executor::block_on(read_request.send().promise)?;
        let out = res.get()?.get_result()?;
        for c in out {
            print!("{}", *c as char)
        }
        return Ok(())
    }
}