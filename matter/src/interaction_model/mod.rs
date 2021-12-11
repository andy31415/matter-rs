use std::any::Any;

use std::sync::Arc;

use crate::{error::Error, tlv::TLVElement, utils::writebuf::WriteBuf};

#[derive(PartialEq)]
pub enum TransactionState {
    Ongoing,
    Complete,
}
pub struct Transaction {
    pub state: TransactionState,
    pub data: Option<Box<dyn Any>>,
}

#[derive(Debug)]
pub struct CmdPathIb {
    /* As per the spec these should be U16, U32, and U16 respectively */
    pub endpoint: Option<u8>,
    pub cluster: Option<u8>,
    pub command: u8,
}

pub struct CommandReq<'a, 'b> {
    pub cmd_path_ib: CmdPathIb,
    pub data: TLVElement<'a>,
    pub resp_buf: &'a mut WriteBuf<'b>,
    pub trans: &'a mut Transaction,
}

pub trait HandleInteraction {
    fn handle_invoke_cmd(&self, cmd_req: &mut CommandReq) -> Result<(), Error>;
}

pub struct InteractionModel {
    handler: Arc<dyn HandleInteraction>,
}
pub mod command;
pub mod demux;
