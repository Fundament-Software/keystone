use crate::http_capnp::response;
use capnp::serialize;

pub fn write_sample_content() -> ::capnp::Result<()> {
    let mut message = ::capnp::message::Builder::new_default();
    {
        let mut response = message.init_root::<response::Builder>();
        response.set_content("Hello World\n");
    }
    println!("haha");
    serialize::write_message(&mut std::io::stdout(), &message);
    println!("hehe");
    serialize::write_message(&mut std::io::stdout(), &message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample() {
        write_sample_content();
    }
}
