use std::path::Path;

use cap_std::fs::{Dir, DirBuilder, File, Metadata, ReadDir, Permissions};
use cap_tempfile::{TempFile, TempDir};
use capnp::{capability::{Promise, Response}, ErrorKind, Error};
use capnp_rpc::pry;
use tokio::io::{AsyncRead, AsyncReadExt};
use crate::{cap_std_capnp::{ambient_authority, cap_fs, dir, dir_builder, dir_entry, dir_options, duration, file, file_type, instant, metadata, monotonic_clock, open_options, permissions, project_dirs, read_dir, system_clock, system_time, system_time_error, temp_dir, temp_file, user_dirs}, spawn::unix_process::UnixProcessServiceSpawnImpl};

pub struct CapFsImpl;

impl cap_fs::Server for CapFsImpl {
    fn use_ambient_authority(&mut self, _: cap_fs::UseAmbientAuthorityParams, mut result: cap_fs::UseAmbientAuthorityResults) -> Promise<(), Error> {
        result.get().set_ambient_authority(capnp_rpc::new_client(AmbientAuthorityImpl{}));
        Promise::ok(())
    }
    fn dir_open(&mut self, params: cap_fs::DirOpenParams, mut result: cap_fs::DirOpenResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = Path::new(pry!(path_reader.get_path()));
        let Ok(file) = std::fs::File::open(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to open file")});
        };
        let dir = DirImpl{dir: Dir::from_std_file(file)};
        result.get().set_dir(capnp_rpc::new_client(dir));
        Promise::ok(())
    }
    fn dir_builder_new(&mut self, _: cap_fs::DirBuilderNewParams, mut result: cap_fs::DirBuilderNewResults) -> Promise<(), Error> {
        result.get().set_builder(capnp_rpc::new_client(DirBuilder::new()));
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

}

pub struct DirImpl {
    dir: Dir
}


impl dir::Server for DirImpl {
    fn open(&mut self, params: dir::OpenParams, mut result: dir::OpenResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_file) = self.dir.open(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to open file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn open_with(&mut self, _: dir::OpenWithParams, _: dir::OpenWithResults) -> Promise<(), Error> {
        todo!()
    }
    fn create_dir(&mut self, params: dir::CreateDirParams, _: dir::CreateDirResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_dir) = self.dir.create_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create dir")});
        };
        Promise::ok(())
    }
    fn create_dir_all(&mut self, params: dir::CreateDirAllParams, _: dir::CreateDirAllResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_dir) = self.dir.create_dir_all(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create dir(all)")});
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
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to open a file in write only mode")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file: _file}));
        Promise::ok(())
    }
    fn canonicalize(&mut self, params: dir::CanonicalizeParams, mut result: dir::CanonicalizeResults) -> Promise<(), Error> {
        let path_reader = pry!(params.get());
        let path = pry!(path_reader.get_path());
        let Ok(_pathBuf) = self.dir.canonicalize(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to canonicalize path")});
        };
        let Some(_str) = _pathBuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Path contains non utf-8 characters")});
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
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to copy file contents")});
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
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create hard link")});
        };
        Promise::ok(())
    }
    fn metadata(&mut self, params: dir::MetadataParams, mut result: dir::MetadataResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_metadata) = self.dir.metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to get file metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn dir_metadata(&mut self, _: dir::DirMetadataParams, mut result: dir::DirMetadataResults) -> Promise<(), Error> {
        let Ok(_metadata) = self.dir.dir_metadata() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to get dir metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn entries(&mut self, _: dir::EntriesParams, mut result: dir::EntriesResults) -> Promise<(), Error> {
        let Ok(_iter) = self.dir.entries() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to get dir entries")});
        };
        result.get().set_iter(capnp_rpc::new_client(ReadDirImpl{iter :_iter}));
        Promise::ok(())
    }
    fn read_dir(&mut self, params: dir::ReadDirParams, mut result: dir::ReadDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_iter) = self.dir.read_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to read dir")});
        };
        result.get().set_iter(capnp_rpc::new_client(ReadDirImpl{iter :_iter}));
        Promise::ok(())
    }
    fn read(&mut self, params: dir::ReadParams, mut result: dir::ReadResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(vec) = self.dir.read(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to read file")});
        };
        result.get().set_result(vec.as_slice());
        Promise::ok(())
    }
    fn read_link(&mut self, params: dir::ReadLinkParams, mut result: dir::ReadLinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_pathbuf) = self.dir.read_link(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to read link")});
        };
        let Some(_str) = _pathbuf.to_str() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to read link")});
        };
        result.get().set_result(_str);
        Promise::ok(())
    }
    fn read_to_string(&mut self, params: dir::ReadToStringParams, mut result: dir::ReadToStringResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(string) = self.dir.read_to_string(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to read file to string")});
        };
        result.get().set_result(string.as_str());
        Promise::ok(())
    }
    fn remove_dir(&mut self, params: dir::RemoveDirParams, _: dir::RemoveDirResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(()) = self.dir.remove_dir(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to remove dir")});
        };
        Promise::ok(())
    }
    fn remove_dir_all(&mut self, params: dir::RemoveDirAllParams, _: dir::RemoveDirAllResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(()) = self.dir.remove_dir_all(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to remove dir(all)")});
        };
        Promise::ok(())
    }
    fn remove_open_dir(&mut self, _: dir::RemoveOpenDirParams, _: dir::RemoveOpenDirResults) -> Promise<(), Error> {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create an owned dir")});
        };
        let Ok(()) = this.remove_open_dir() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to remove open dir")});
        };
        Promise::ok(())
    }
    fn remove_open_dir_all(&mut self, _: dir::RemoveOpenDirAllParams, _: dir::RemoveOpenDirAllResults) -> Promise<(), Error> {
        //Original function consumes self so that it can't be used again, not sure how to do that with capnproto
        let Ok(this) = self.dir.try_clone() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create an owned dir")});
        };
        let Ok(()) = this.remove_open_dir_all() else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to remove open dir(all)")});
        };
        Promise::ok(())
    }
    fn remove_file(&mut self, params: dir::RemoveFileParams, _: dir::RemoveFileResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(()) = self.dir.remove_file(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to remove file")});
        };
        Promise::ok(())
    }
    fn rename(&mut self, params: dir::RenameParams, _: dir::RenameResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let from = pry!(params_reader.get_from());
        let to = pry!(params_reader.get_to());
        let this = &self.dir;
        let Ok(()) = self.dir.rename(from, this, to) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to rename file")});
        };
        Promise::ok(())
    }
    fn set_permissions(&mut self, params: dir::SetPermissionsParams, _: dir::SetPermissionsResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let perm = pry!(params_reader.get_perm());

        //let Ok(()) = self.dir.set_permissions(path, perm) else {
        //    return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to set permissions")});
        //};
        //Promise::ok(())
        todo!()
    }
    fn symlink_metadata(&mut self, params: dir::SymlinkMetadataParams, mut result: dir::SymlinkMetadataResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let Ok(_metadata) = self.dir.symlink_metadata(path) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to get symlink metadata")});
        };
        result.get().set_metadata(capnp_rpc::new_client(MetadataImpl{metadata: _metadata}));
        Promise::ok(())
    }
    fn write(&mut self, params: dir::WriteParams, _: dir::WriteResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let path = pry!(params_reader.get_path());
        let contents = pry!(params_reader.get_contents());
        let Ok(()) = self.dir.write(path, contents) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to write to file")});
        };
        Promise::ok(())
    }
    fn symlink(&mut self, params: dir::SymlinkParams, _: dir::SymlinkResults) -> Promise<(), Error> {
        let params_reader = pry!(params.get());
        let original = pry!(params_reader.get_original());
        let link = pry!(params_reader.get_link());
        //let Ok(()) = self.dir.symlink() else {
        //
        //};

        todo!()
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
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to check if entity exists")});
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
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create temp dir")});
        };
        //result.get().set_temp_dir(capnp_rpc::new_client(temp_dir));
        todo!()
    }
    fn temp_file_new(&mut self,  _: dir::TempFileNewParams, mut result: dir::TempFileNewResults) -> Promise<(), Error> {
        let Ok(temp_file) = cap_tempfile::TempFile::new(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create temp file")});
        };
        result.get().set_temp_file(capnp_rpc::new_client(temp_file));
        Promise::ok(())
    }
    fn temp_file_new_anonymous(&mut self, _: dir::TempFileNewAnonymousParams, mut result: dir::TempFileNewAnonymousResults) -> Promise<(), Error> {
        let Ok(file) = cap_tempfile::TempFile::new_anonymous(&self.dir) else {
            return Promise::err(Error{kind: capnp::ErrorKind::Failed, description: String::from("Failed to create temp anonymous temp file")});
        };
        result.get().set_file(capnp_rpc::new_client(FileImpl{file}));
        Promise::ok(())
    }*/
}

pub struct ReadDirImpl {
    iter: ReadDir
}

impl read_dir::Server for ReadDirImpl {
    
}

pub struct FileImpl {
    file: File
}

impl file::Server for FileImpl {

}

pub struct MetadataImpl {
    metadata: Metadata
}

impl metadata::Server for MetadataImpl {

}

pub struct PermissionsImpl {
    permissions: Permissions
}

impl permissions::Server for PermissionsImpl {

}
//impl temp_dir::Server for TempDir {
//
//}

impl temp_file::Server for TempFile<'_> {

}

impl dir_builder::Server for DirBuilder {

}