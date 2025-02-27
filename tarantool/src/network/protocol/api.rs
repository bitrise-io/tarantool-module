use std::io::{Cursor, Write};

use super::Error;
use crate::tuple::{ToTupleBuffer, Tuple};

use super::codec::IProtoType;
use super::{codec, SyncIndex};

pub trait Request {
    const TYPE: IProtoType;
    type Response: Sized;

    fn encode_header(&self, out: &mut impl Write, sync: SyncIndex) -> Result<(), Error> {
        codec::encode_header(out, sync, Self::TYPE)
    }

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error>;

    fn encode(&self, out: &mut impl Write, sync: SyncIndex) -> Result<(), Error> {
        self.encode_header(out, sync)?;
        self.encode_body(out)?;
        Ok(())
    }

    fn decode_body(&self, r#in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error>;
}

// TODO: Implement `Request` for other types in `IProtoType`

pub struct Ping;

impl Request for Ping {
    const TYPE: IProtoType = IProtoType::Ping;
    type Response = ();

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error> {
        codec::encode_ping(out)
    }

    fn decode_body(&self, _in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error> {
        Ok(())
    }
}

pub struct Call<'a, 'b, T> {
    pub fn_name: &'a str,
    pub args: &'b T,
}

impl<'a, 'b, T: ToTupleBuffer> Request for Call<'a, 'b, T> {
    const TYPE: IProtoType = IProtoType::Call;
    type Response = Option<Tuple>;

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error> {
        codec::encode_call(out, self.fn_name, self.args)
    }

    fn decode_body(&self, r#in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error> {
        codec::decode_call(r#in)
    }
}

pub struct Eval<'a, 'b, T> {
    pub expr: &'a str,
    pub args: &'b T,
}

impl<'a, 'b, T: ToTupleBuffer> Request for Eval<'a, 'b, T> {
    const TYPE: IProtoType = IProtoType::Eval;
    type Response = Option<Tuple>;

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error> {
        codec::encode_eval(out, self.expr, self.args)
    }

    fn decode_body(&self, r#in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error> {
        codec::decode_call(r#in)
    }
}

pub struct Execute<'a, 'b, T> {
    pub sql: &'a str,
    pub bind_params: &'b T,
    pub limit: Option<usize>,
}

impl<'a, 'b, T: ToTupleBuffer> Request for Execute<'a, 'b, T> {
    const TYPE: IProtoType = IProtoType::Execute;
    type Response = Vec<Tuple>;

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error> {
        codec::encode_execute(out, self.sql, self.bind_params)
    }

    fn decode_body(&self, r#in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error> {
        codec::decode_multiple_rows(r#in, self.limit)
    }
}

pub struct Auth<'u, 'p, 's> {
    pub user: &'u str,
    pub pass: &'p str,
    pub salt: &'s [u8],
}

impl<'u, 'p, 's> Request for Auth<'u, 'p, 's> {
    const TYPE: IProtoType = IProtoType::Auth;
    type Response = ();

    fn encode_body(&self, out: &mut impl Write) -> Result<(), Error> {
        codec::encode_auth(out, self.user, self.pass, self.salt)
    }

    fn decode_body(&self, _in: &mut Cursor<Vec<u8>>) -> Result<Self::Response, Error> {
        Ok(())
    }
}
