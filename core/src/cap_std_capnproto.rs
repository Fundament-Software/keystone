use std::{path::Path};

use cap_std::{fs::{Dir, DirBuilder, File, Metadata, ReadDir, Permissions, OpenOptions, DirEntry, FileType}, time::{MonotonicClock, SystemClock, SystemTime, Duration, Instant}};
use cap_tempfile::{TempFile, TempDir};
use cap_directories::{self, UserDirs, ProjectDirs};
use capnp::{capability::{Promise, Response}, ErrorKind, Error, io::Write};
use capnp_rpc::pry;
use capnp_macros::capnp_let;
use tokio::io::{AsyncRead, AsyncReadExt};
use crate::{cap_std_capnp::{ambient_authority, cap_fs, dir, dir_builder, dir_entry, dir_options, duration, file, file_type, instant, metadata, monotonic_clock, open_options, permissions, project_dirs, read_dir, system_clock, system_time, system_time_error, temp_dir, temp_file, user_dirs}, spawn::unix_process::UnixProcessServiceSpawnImpl, byte_stream::ByteStreamImpl};
//use capnp::IntoResult;

pub struct CapFsImpl;

impl cap_fs::Server for CapFsImpl {
    fn use_ambient_authority(&mut self, _: cap_fs::UseAmbientAuthorityParams, mut result: cap_fs::UseAmbientAuthorityResults) -> Promise<(), Error> {
        result.get().set_ambient_authority(capnp_rpc::new_client(AmbientAuthorityImpl{}));
        Promise::ok(())
    }
    fn dir_open(&mut self, params: cap_fs::DirOpenParams, mut result: cap_fs::DirOpenResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(file) = std::fs::File::open(Path::new(path)) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file")});
        };
        let dir = DirImpl{dir: Dir::from_std_file(file)};
        result.get().set_dir(capnp_rpc::new_client(dir));
        Promise::ok(())
    }
    fn dir_builder_new(&mut self, _: cap_fs::DirBuilderNewParams, mut result: cap_fs::DirBuilderNewResults) -> Promise<(), Error> {
        result.get().set_builder(capnp_rpc::new_client(DirBuilderImpl{dir_builder: DirBuilder::new()}));
        Promise::ok(())
    }
    fn options(&mut self, _: cap_fs::OptionsParams, mut result: cap_fs::OptionsResults) -> Promise<(), Error> {
        let _options = File::options();
        result.get().set_options(capnp_rpc::new_client(OpenOptionsImpl{open_options: _options}));
        Promise::ok(())
    }
    /*
    fn temp_dir_new_in(&mut self, params: cap_fs::TempDirNewInParams, mut result: cap_fs::TempDirNewInResults) -> Promise<(), capnp::Error> {
        let dir_reader = pry!(params.get());
        let dir = pry!(dir_reader.get_dir());
        //cap_tempfile::TempDir::new_in(dir);
        todo!()
    }
    fn temp_file_new(&mut self,  params: cap_fs::TempFileNewParams, mut result: cap_fs::TempFileNewResults) -> Promise<(), capnp::Error> {
        let dir_reader = pry!(params.get());
        let dir = pry!(dir_reader.get_dir());

        todo!()
    }
    fn temp_file_new_anonymous(&mut self, params: cap_fs::TempFileNewAnonymousParams, mut result: cap_fs::TempFileNewAnonymousResults) -> Promise<(), capnp::Error> {
        let dir_reader = pry!(params.get());
        let dir = pry!(dir_reader.get_dir());

        todo!()
    }*/
}

pub struct AmbientAuthorityImpl;

impl ambient_authority::Server for AmbientAuthorityImpl {
    fn file_open_ambient(&mut self, params: ambient_authority::FileOpenAmbientParams, mut result: ambient_authority::FileOpenAmbientResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_file) = File::open_ambient(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file using ambient authority")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn file_create_ambient(&mut self, params: ambient_authority::FileCreateAmbientParams, mut result: ambient_authority::FileCreateAmbientResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_file) = File::create_ambient(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create file using ambient authority")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn file_open_ambient_with(&mut self, params: ambient_authority::FileOpenAmbientWithParams, mut result: ambient_authority::FileOpenAmbientWithResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let read = params_reader.get_read();
        let write = params_reader.get_write();
        let append = params_reader.get_append();
        let truncate = params_reader.get_truncate();
        let create = params_reader.get_create();
        let create_new = params_reader.get_create_new();
        let mut options = OpenOptions::new();
        let path = pry!(params_reader.get_path());
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
        let path = pry!(params_reader.get_path());
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_dir) = Dir::open_ambient_dir(path, ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open dir using ambient authority")});
        };
        result.get().set_result(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }/*
    fn dir_open_parent(&mut self, _: ambient_authority::DirOpenParentParams, mut result: ambient_authority::DirOpenParentResults) -> Promise<(), Error> {
        let ambient_authority = cap_std::ambient_authority();
        let Ok(_dir) = Dir::open_parent_dir(ambient_authority) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open parent dir using ambient authority")});
        };
        result.get().set_result(capnp_rpc::new_client(DirImpl{dir: _dir}));
        todo!()
    }*/
    fn dir_create_ambient_all(&mut self, params: ambient_authority::DirCreateAmbientAllParams, mut result: ambient_authority::DirCreateAmbientAllResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
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
        let qualifier = pry!(params_reader.get_qualifier());
        let organization = pry!(params_reader.get_organization());
        let application = pry!(params_reader.get_application());
        let ambient_authority = cap_std::ambient_authority();
        let Some(_project_dirs) = ProjectDirs::from(qualifier, organization, application, ambient_authority) else {
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }/*
    fn temp_dir_new(&mut self, _: ambient_authority::TempDirNewParams, mut result: ambient_authority::TempDirNewResults) -> Promise<(), Error> {
        
        result.get().set_temp_dir(capnp_rpc::new_client(...));
        todo!()
    }*/
}

pub struct DirImpl {
    dir: Dir
}


impl dir::Server for DirImpl {
    fn open(&mut self, params: dir::OpenParams, mut result: dir::OpenResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_file) = self.dir.open(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn open_with(&mut self, params: dir::OpenWithParams, mut result: dir::OpenWithResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let read = params_reader.get_read();
        let write = params_reader.get_write();
        let append = params_reader.get_append();
        let truncate = params_reader.get_truncate();
        let create = params_reader.get_create();
        let create_new = params_reader.get_create_new();
        let mut options = OpenOptions::new();
        let path = pry!(params_reader.get_path());
        options.read(read).write(write).append(append).truncate(truncate).create(create).create_new(create_new);
        let Ok(_file) = self.dir.open_with(path, &options) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open file for reading(With custom options)")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn create_dir(&mut self, params: dir::CreateDirParams, _: dir::CreateDirResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_dir) = self.dir.create_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create dir")});
        };
        Promise::ok(())
    }
    fn create_dir_all(&mut self, params: dir::CreateDirAllParams, _: dir::CreateDirAllResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_dir) = self.dir.create_dir_all(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create dir(all)")});
        };
        Promise::ok(())
    }
    fn create_dir_with(&mut self, _: dir::CreateDirWithParams, _: dir::CreateDirWithResults) -> Promise<(), Error> {
        todo!()
    }
    fn create(&mut self, params: dir::CreateParams, mut result: dir::CreateResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_file) = self.dir.create(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to open a file in write only mode")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn canonicalize(&mut self, params: dir::CanonicalizeParams, mut result: dir::CanonicalizeResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_pathBuf) = self.dir.canonicalize(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to canonicalize path")});
        };
        let Some(_str) = _pathBuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Path contains non utf-8 characters")});
        };
        result.get().set_path_buf(_str);
        Promise::ok(())
    }
    fn copy(&mut self, params: dir::CopyParams, mut result: dir::CopyResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let from = pry!(params_reader.get_path_from()) ;
        let to = pry!(params_reader.get_path_to());
        let this = &self.dir;
        let Ok(bytes) = self.dir.copy(from, this, to) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to copy file contents")});
        };
        result.get().set_result(bytes);
        Promise::ok(())
    }
    fn hard_link(&mut self, params: dir::HardLinkParams, _: dir::HardLinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let src = pry!(params_reader.get_src_path());
        let dst = pry!(params_reader.get_dst_path());
        let this = &self.dir;
        let Ok(()) = self.dir.hard_link(src, this, dst) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create hard link")});
        };
        Promise::ok(())
    }
    fn metadata(&mut self, params: dir::MetadataParams, mut result: dir::MetadataResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
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
        let path = pry!(params_reader.get_path());
        let Ok(mut _iter) = self.dir.read_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read dir")});
        };
        result.get().set_iter(capnp_rpc::new_client(ReadDirImpl{iter:_iter}));
        Promise::ok(())
    }
    fn read(&mut self, params: dir::ReadParams, mut result: dir::ReadResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(vec) = self.dir.read(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read file")});
        };
        result.get().set_result(vec.as_slice());
        Promise::ok(())
    }
    fn read_link(&mut self, params: dir::ReadLinkParams, mut result: dir::ReadLinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_pathbuf) = self.dir.read_link(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read link")});
        };
        let Some(_str) = _pathbuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read link")});
        };
        result.get().set_result(_str);
        Promise::ok(())
    }
    fn read_to_string(&mut self, params: dir::ReadToStringParams, mut result: dir::ReadToStringResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(string) = self.dir.read_to_string(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to read file to string")});
        };
        result.get().set_result(string.as_str());
        Promise::ok(())
    }
    fn remove_dir(&mut self, params: dir::RemoveDirParams, _: dir::RemoveDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(()) = self.dir.remove_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove dir")});
        };
        Promise::ok(())
    }
    fn remove_dir_all(&mut self, params: dir::RemoveDirAllParams, _: dir::RemoveDirAllResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
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
        let path = pry!(params_reader.get_path());
        let Ok(()) = self.dir.remove_file(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to remove file")});
        };
        Promise::ok(())
    }
    fn rename(&mut self, params: dir::RenameParams, _: dir::RenameResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let from = pry!(params_reader.get_from());
        let to = pry!(params_reader.get_to());
        let this = &self.dir;
        let Ok(()) = self.dir.rename(from, this, to) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to rename file")});
        };
        Promise::ok(())
    }
    fn set_readonly(&mut self, params: dir::SetReadonlyParams, _: dir::SetReadonlyResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
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
        let path = pry!(params_reader.get_path());
        let Ok(_metadata) = self.dir.symlink_metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get symlink metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn write(&mut self, params: dir::WriteParams, _: dir::WriteResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let contents = pry!(params_reader.get_contents());
        let Ok(()) = self.dir.write(path, contents) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to write to file")});
        };
        Promise::ok(())
    }
    fn symlink(&mut self, params: dir::SymlinkParams, _: dir::SymlinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let original = pry!(params_reader.get_original());
        let link = pry!(params_reader.get_link());
        let Ok(()) = self.dir.symlink_dir(original, link) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create symlink")});
        };
        Promise::ok(())
    }
    fn exists(&mut self, params: dir::ExistsParams, mut result: dir::ExistsResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let _result = self.dir.exists(path);
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn try_exists(&mut self, params: dir::TryExistsParams, mut result: dir::TryExistsResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_result) = self.dir.try_exists(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to check if entity exists")});
        };
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_file(&mut self, params: dir::IsFileParams, mut result: dir::IsFileResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let _result = self.dir.is_file(path);
        result.get().set_result(_result);
        Promise::ok(())
    }
    fn is_dir(&mut self, params: dir::IsDirParams, mut result: dir::IsDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let _result = self.dir.is_dir(path);
        result.get().set_result(_result);
        Promise::ok(())
    }

/*
    fn temp_dir_new_in(&mut self, _: dir::TempDirNewInParams, mut result: dir::TempDirNewInResults) -> Promise<(), Error> {
        let Ok(temp_dir) = cap_tempfile::TempDir::new_in(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp dir")});
        };
        //result.get().set_temp_dir(capnp_rpc::new_client(temp_dir));
        todo!()
    }
    fn temp_file_new(&mut self,  _: dir::TempFileNewParams, mut result: dir::TempFileNewResults) -> Promise<(), Error> {
        let Ok(temp_file) = cap_tempfile::TempFile::new(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp file")});
        };
        result.get().set_temp_file(capnp_rpc::new_client(temp_file));
        Promise::ok(())
    }
    fn temp_file_new_anonymous(&mut self, _: dir::TempFileNewAnonymousParams, mut result: dir::TempFileNewAnonymousResults) -> Promise<(), Error> {
        let Ok(file) = cap_tempfile::TempFile::new_anonymous(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to create temp anonymous temp file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file}));
        Promise::ok(())
    }*/
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
        let read = params_reader.get_read();
        let write = params_reader.get_write();
        let append = params_reader.get_append();
        let truncate = params_reader.get_truncate();
        let create = params_reader.get_create();
        let create_new = params_reader.get_create_new();
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
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
        result.get().set_type(capnp_rpc::new_client(FileTypeImpl{file_type: _file_type}));
        Promise::ok(())
    }
    fn file_name(&mut self, _: dir_entry::FileNameParams, mut result: dir_entry::FileNameResults) -> Promise<(), Error> {
        let _name = self.entry.file_name();
        let Some(_name) = _name.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("File name not valid utf-8")});
        };
        result.get().set_result(_name);
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
        //Not completely sure that's how byte streams work, also probably needs to use async/futures potentially rc

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
        let _type = self.metadata.file_type();
        result.get().set_file_type(capnp_rpc::new_client(FileTypeImpl{file_type: _type}));
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

pub struct FileTypeImpl {
    file_type: FileType
}

impl file_type::Server for FileTypeImpl {
    fn dir(&mut self, _: file_type::DirParams, mut result: file_type::DirResults) -> Promise<(), Error> {
        let ft = FileType::dir();
        result.get().set_file_type(capnp_rpc::new_client(FileTypeImpl{file_type: ft}));
        Promise::ok(())
    }
    fn file(&mut self, _: file_type::FileParams, mut result: file_type::FileResults) -> Promise<(), Error> {
        let ft = FileType::file();
        result.get().set_file_type(capnp_rpc::new_client(FileTypeImpl{file_type: ft}));
        Promise::ok(())
    }
    fn unknown(&mut self, _: file_type::UnknownParams, mut result: file_type::UnknownResults) -> Promise<(), Error> {
        let ft = FileType::unknown();
        result.get().set_file_type(capnp_rpc::new_client(FileTypeImpl{file_type: ft}));
        Promise::ok(())
    }
    fn is_dir(&mut self, _: file_type::IsDirParams, mut result: file_type::IsDirResults) -> Promise<(), Error> {
        let is_dir = self.file_type.is_dir();
        result.get().set_result(is_dir);
        Promise::ok(())
    }
    fn is_file(&mut self, _: file_type::IsFileParams, mut result: file_type::IsFileResults) -> Promise<(), Error> {
        let is_file = self.file_type.is_file();
        result.get().set_result(is_file);
        Promise::ok(())
    }
    fn is_symlink(&mut self, _: file_type::IsSymlinkParams, mut result: file_type::IsSymlinkResults) -> Promise<(), Error> {
        let is_symlink = self.file_type.is_symlink();
        result.get().set_result(is_symlink);
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
/*
pub struct TempDirImpl {
    temp_dir: TempDir
}

impl temp_dir::Server for TempDirImpl {

}*/
pub struct TempFileImpl<'a> {
    temp_file: TempFile<'a>
}

impl temp_file::Server for TempFileImpl<'_> {
    fn as_file(&mut self, _: temp_file::AsFileParams, mut result: temp_file::AsFileResults) -> Promise<(), Error> {
        let _file = self.temp_file.as_file_mut();
        let Ok(_cloned) = _file.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to get an owned version of the underlying file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _cloned}));
        Promise::ok(())
    }/*
    fn replace(&mut self, params: temp_file::ReplaceParams, _: temp_file::ReplaceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let dest = pry!(params_reader.get_dest());
        let Ok(()) = self.temp_file.replace(dest) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to write file to the target location")});
        };
        Promise::ok(())
    }*/
}

pub struct DirBuilderImpl {
    dir_builder: DirBuilder
}

impl dir_builder::Server for DirBuilderImpl {

}

pub struct OpenOptionsImpl {
    open_options: OpenOptions
}

impl open_options::Server for OpenOptionsImpl {

}

pub struct SystemTimeImpl {
    system_time: SystemTime
}

impl system_time::Server for SystemTimeImpl {
    fn duration_since(&mut self, params: system_time::DurationSinceParams, mut result: system_time::DurationSinceResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        //let dur = params_reader.get_duration_since_unix_epoch();
        //capnp_let!({duration_since_unix_epoch : {secs, nanos}} = params_reader);
        capnp_let!({duration_since_unix_epoch : {secs, nanos}} = params_reader);
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
    /*fn duration_since(&mut self, _: DurationSinceParams, _: DurationSinceResults) -> Promise<(), Error> {
        
    }
    fn checked_duration_since(&mut self, _: CheckedDurationSinceParams, _: CheckedDurationSinceResults) -> Promise<(), Error> {
        
    }
    fn saturating_duration_since(&mut self, _: SaturatingDurationSinceParams, _: SaturatingDurationSinceResults) -> Promise<(), Error> {
        
    }*/
    fn checked_add(&mut self, params: instant::CheckedAddParams, mut result: instant::CheckedAddResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let duration_reader = pry!(params_reader.get_duration());
        let secs = duration_reader.get_secs();
        let nanos = duration_reader.get_nanos();
        let duration = Duration::new(secs, nanos);
        let Some(_instant) = self.instant.checked_add(duration) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to add duration to instant")});
        };
        result.get().set_instant(capnp_rpc::new_client(InstantImpl{instant: _instant}));
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
        result.get().set_instant(capnp_rpc::new_client(InstantImpl{instant: _instant}));
        Promise::ok(())
    }
}

pub struct MonotonicClockImpl {
    monotonic_clock: MonotonicClock
}

impl monotonic_clock::Server for MonotonicClockImpl {
    fn now(&mut self, _: monotonic_clock::NowParams, mut result: monotonic_clock::NowResults) -> Promise<(), Error> {
        let _instant = self.monotonic_clock.now();
        result.get().set_instant(capnp_rpc::new_client(InstantImpl{instant: _instant}));
        Promise::ok(())
    }
    //fn elapsed(&mut self, _: monotonic_clock::ElapsedParams, _: monotonic_clock::ElapsedResults) -> Promise<(), Error> {
//
    //}
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
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }
    fn config_dir( &mut self, _: project_dirs::ConfigDirParams, mut result: project_dirs::ConfigDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.config_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve config directory")});
        };
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }
    fn data_dir(&mut self, _: project_dirs::DataDirParams, mut result: project_dirs::DataDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.data_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve data directory")});
        };
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }
    fn data_local_dir(&mut self, _: project_dirs::DataLocalDirParams, mut result: project_dirs::DataLocalDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.data_local_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve local data directory")});
        };
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }
    fn runtime_dir(&mut self, _: project_dirs::RuntimeDirParams, mut result: project_dirs::RuntimeDirResults) -> Promise<(), Error> {
        let Ok(_dir) = self.project_dirs.runtime_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, extra: String::from("Failed to retrieve runtime directory")});
        };
        result.get().set_dir(capnp_rpc::new_client(DirImpl{dir: _dir}));
        Promise::ok(())
    }
}

pub type Result<T> = ::core::result::Result<T, Error>;

pub trait IntoResult {
    type InnerType;
    fn into_result(self) -> Result<Self::InnerType>;
}

impl<T> IntoResult for Result<T> {
    type InnerType = T;
    fn into_result(self) -> Result<Self::InnerType> {
        self
    }
}

impl IntoResult for u64 {
    type InnerType = u64;
    fn into_result(self) -> Result<Self::InnerType> {
        Ok(self)
    }
}

impl IntoResult for u32 {
    type InnerType = u32;
    fn into_result(self) -> Result<Self::InnerType> {
        Ok(self)
    }
}
/* 
impl<T: capnp::introspect::Introspect> IntoResult for T {
    type InnerType = Self;
    fn into_result(self) -> Result<Self::InnerType> {
        Ok::<Self::InnerType, Error>(self)
    }
}*/

#[cfg(test)]
mod tests {
    use std::{path::Path};

    use cap_std::{fs::{Dir, DirBuilder, File, Metadata, ReadDir, Permissions, OpenOptions, DirEntry, FileType}, time::{MonotonicClock, SystemClock, SystemTime, Duration, Instant}};
    use cap_tempfile::{TempFile, TempDir};
    use cap_directories::{self, UserDirs, ProjectDirs};
    use capnp_rpc::pry;
    use capnp_macros::capnp_let;
    use tokio::io::{AsyncRead, AsyncReadExt};
    use crate::{cap_std_capnp::{ambient_authority, cap_fs, dir, dir_builder, dir_entry, dir_options, duration, file, file_type, instant, metadata, monotonic_clock, open_options, permissions, project_dirs, read_dir, system_clock, system_time, system_time_error, temp_dir, temp_file, user_dirs}, spawn::unix_process::UnixProcessServiceSpawnImpl, byte_stream::ByteStreamImpl};
    

    #[test]
    fn cap_std_test_open_read() -> eyre::Result<()> {
        
        use std::io::{BufWriter, Write};
        use super::FileImpl;

        let mut path = std::env::temp_dir();
        path.push("capnp_test.txt");
        let _f = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(_f);
        writer.write_all(b"Just a test file ")?;
        writer.flush()?;

        let mut path = std::env::temp_dir();
        let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap();
        let out = dir.read("capnp_test.txt")?;
        for c in out {
            print!("{}", c as char)
        }
        return Ok(())
    }
    #[test]
    fn test_write() -> eyre::Result<()> {
        //use ambient authority to open a dir, create a file(Or open it in write mode if it already exists), open a bytestream, use the bytestream to write some bytes
        let cap: cap_fs::Client = capnp_rpc::new_client(crate::cap_std_capnproto::CapFsImpl);

        let mut use_aa_request = cap.use_ambient_authority_request();
        let res = futures::executor::block_on(use_aa_request.send().promise);
        let ambient_authority = res?.get()?.get_ambient_authority()?;

        let mut request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        request.get().set_path(path.to_str().unwrap());
        let result = futures::executor::block_on(request.send().promise);
        let dir = result?.get()?.get_result()?;

        let mut create_request = dir.create_request();
        create_request.get().set_path("capnp_test.txt");
        let res = futures::executor::block_on(create_request.send().promise)?;
        let file = res.get()?.get_file()?;

        let mut open_bytestream_request = file.open_request();
        let res = futures::executor::block_on(open_bytestream_request.send().promise)?;
        let stream = res.get()?.get_stream()?;

        let mut write_request = stream.write_request();
        write_request.get().set_bytes(b" Writing some bytes test ");
        let _res = futures::executor::block_on(write_request.send().promise)?;
        return Ok(())
    }
    #[test]
    fn test_open_read() -> eyre::Result<()> {
        //use ambient authority to open directory, read contents of a file as bytes and print them out
        use std::io::{BufWriter, Write};
        use super::FileImpl;

        let mut path = std::env::temp_dir();
        path.push("capnp_test.txt");
        let _f = std::fs::File::create(path)?;
        let mut writer = BufWriter::new(_f);
        writer.write_all(b"Just a test file ")?;
        writer.flush()?;

        let cap: cap_fs::Client = capnp_rpc::new_client(crate::cap_std_capnproto::CapFsImpl);
        let mut request = cap.use_ambient_authority_request();
        let r = futures::executor::block_on(request.send().promise);
        let ambient_authority = r?.get()?.get_ambient_authority()?;
        
        let mut request = ambient_authority.dir_open_ambient_request();
        let mut path = std::env::temp_dir();
        request.get().set_path(path.to_str().unwrap());
        let result = futures::executor::block_on(request.send().promise)?;
        let dir = result.get()?.get_result()?;

        let mut read_request = dir.read_request();
        read_request.get().set_path("capnp_test.txt");
        let res = futures::executor::block_on(read_request.send().promise)?;
        let out = res.get()?.get_result()?;
        for c in out {
            print!("{}", *c as char)
        }
        return Ok(())
    }









}