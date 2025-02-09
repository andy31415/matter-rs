use crate::cmd_enter;
use crate::data_model::objects::*;
use crate::data_model::sdm::failsafe::FailSafe;
use crate::interaction_model::core::IMStatusCode;
use crate::interaction_model::messages::ib;
use crate::tlv::{FromTLV, TLVElement, TLVWriter, TagType, ToTLV};
use crate::{error::*, interaction_model::command::CommandReq};
use log::info;
use num_derive::FromPrimitive;
use std::sync::Arc;

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum CommissioningError {
    Ok = 0,
    ErrValueOutsideRange = 1,
    ErrInvalidAuth = 2,
    ErrNotCommissioning = 3,
}

pub const ID: u32 = 0x0030;

#[derive(FromPrimitive)]
pub enum Attributes {
    BreadCrumb = 0,
    BasicCommissioningInfo = 1,
    RegConfig = 2,
    LocationCapability = 3,
}

#[derive(FromPrimitive)]
pub enum Commands {
    ArmFailsafe = 0x00,
    ArmFailsafeResp = 0x01,
    SetRegulatoryConfig = 0x02,
    SetRegulatoryConfigResp = 0x03,
    CommissioningComplete = 0x04,
    CommissioningCompleteResp = 0x05,
}

pub enum RegLocationType {
    Indoor = 0,
    Outdoor = 1,
    IndoorOutdoor = 2,
}

fn attr_bread_crumb_new(bread_crumb: u64) -> Result<Attribute, Error> {
    Attribute::new(
        Attributes::BreadCrumb as u16,
        AttrValue::Uint64(bread_crumb),
        Access::READ | Access::WRITE | Access::NEED_ADMIN,
        Quality::NONE,
    )
}

fn attr_reg_config_new(reg_config: RegLocationType) -> Result<Attribute, Error> {
    Attribute::new(
        Attributes::RegConfig as u16,
        AttrValue::Uint8(reg_config as u8),
        Access::RV,
        Quality::NONE,
    )
}

fn attr_location_capability_new(reg_config: RegLocationType) -> Result<Attribute, Error> {
    Attribute::new(
        Attributes::LocationCapability as u16,
        AttrValue::Uint8(reg_config as u8),
        Access::RV,
        Quality::FIXED,
    )
}

fn attr_comm_info_new() -> Result<Attribute, Error> {
    Attribute::new(
        Attributes::BasicCommissioningInfo as u16,
        AttrValue::Custom,
        Access::RV,
        Quality::FIXED,
    )
}

#[derive(FromTLV, ToTLV)]
struct FailSafeParams {
    expiry_len: u8,
    bread_crumb: u8,
}

pub struct GenCommCluster {
    expiry_len: u16,
    failsafe: Arc<FailSafe>,
    base: Cluster,
}

impl ClusterType for GenCommCluster {
    fn base(&self) -> &Cluster {
        &self.base
    }
    fn base_mut(&mut self) -> &mut Cluster {
        &mut self.base
    }

    fn read_custom_attribute(
        &self,
        tag: TagType,
        tw: &mut TLVWriter,
        attr_id: u16,
    ) -> Result<(), IMStatusCode> {
        match num::FromPrimitive::from_u16(attr_id).ok_or(IMStatusCode::UnsupportedAttribute)? {
            Attributes::BasicCommissioningInfo => {
                let _ = tw.start_struct(tag);
                let _ = tw.u16(TagType::Context(0), self.expiry_len);
                let _ = tw.end_container();
                Ok(())
            }
            _ => Err(IMStatusCode::UnsupportedAttribute),
        }
    }

    fn write_attribute(&mut self, data: &TLVElement, attr_id: u16) -> Result<(), IMStatusCode> {
        self.base.write_attribute(data, attr_id)
    }

    fn handle_command(&mut self, cmd_req: &mut CommandReq) -> Result<(), IMStatusCode> {
        let cmd = cmd_req
            .cmd
            .path
            .leaf
            .map(num::FromPrimitive::from_u32)
            .ok_or(IMStatusCode::UnsupportedCommand)?
            .ok_or(IMStatusCode::UnsupportedCommand)?;
        match cmd {
            Commands::ArmFailsafe => self.handle_command_armfailsafe(cmd_req),
            Commands::SetRegulatoryConfig => self.handle_command_setregulatoryconfig(cmd_req),
            Commands::CommissioningComplete => self.handle_command_commissioningcomplete(cmd_req),
            _ => Err(IMStatusCode::UnsupportedCommand),
        }
    }
}

impl GenCommCluster {
    pub fn new() -> Result<Box<Self>, Error> {
        let failsafe = Arc::new(FailSafe::new());

        let mut c = Box::new(GenCommCluster {
            // TODO: Arch-Specific
            expiry_len: 120,
            failsafe,
            base: Cluster::new(ID)?,
        });
        c.base.add_attribute(attr_bread_crumb_new(0)?)?;
        // TODO: Arch-Specific
        c.base
            .add_attribute(attr_reg_config_new(RegLocationType::IndoorOutdoor)?)?;
        // TODO: Arch-Specific
        c.base.add_attribute(attr_location_capability_new(
            RegLocationType::IndoorOutdoor,
        )?)?;
        c.base.add_attribute(attr_comm_info_new()?)?;

        Ok(c)
    }

    pub fn failsafe(&self) -> Arc<FailSafe> {
        self.failsafe.clone()
    }

    fn handle_command_armfailsafe(&mut self, cmd_req: &mut CommandReq) -> Result<(), IMStatusCode> {
        cmd_enter!("ARM Fail Safe");

        let p = FailSafeParams::from_tlv(&cmd_req.data)?;

        if self
            .failsafe
            .arm(p.expiry_len, cmd_req.trans.session.get_session_mode())
            .is_err()
        {
            return Err(IMStatusCode::Busy);
        }

        let cmd_data = |t: &mut TLVWriter| {
            t.u8(TagType::Context(0), CommissioningError::Ok as u8)?;
            t.utf8(TagType::Context(1), b"")
        };
        let resp = ib::InvResp::cmd_new(0, ID, Commands::ArmFailsafeResp as u16, &cmd_data);
        let _ = resp.to_tlv(cmd_req.resp, TagType::Anonymous);
        cmd_req.trans.complete();
        Ok(())
    }

    fn handle_command_setregulatoryconfig(
        &mut self,
        cmd_req: &mut CommandReq,
    ) -> Result<(), IMStatusCode> {
        cmd_enter!("Set Regulatory Config");
        // These data types don't match the spec
        let country_code = cmd_req
            .data
            .find_tag(1)
            .map_err(|_| IMStatusCode::InvalidCommand)?
            .slice()
            .map_err(|_| IMStatusCode::InvalidCommand)?;
        info!("Received country code: {:?}", country_code);

        let cmd_data = |t: &mut TLVWriter| {
            t.u8(TagType::Context(0), 0)?;
            t.utf8(TagType::Context(1), b"")
        };
        let resp = ib::InvResp::cmd_new(0, ID, Commands::SetRegulatoryConfigResp as u16, &cmd_data);
        let _ = resp.to_tlv(cmd_req.resp, TagType::Anonymous);
        cmd_req.trans.complete();
        Ok(())
    }

    fn handle_command_commissioningcomplete(
        &mut self,
        cmd_req: &mut CommandReq,
    ) -> Result<(), IMStatusCode> {
        cmd_enter!("Commissioning Complete");
        let mut status: u8 = CommissioningError::Ok as u8;

        // Has to be a Case Session
        if cmd_req.trans.session.get_local_fabric_idx().is_none() {
            status = CommissioningError::ErrInvalidAuth as u8;
        }

        // AddNOC or UpdateNOC must have happened, and that too for the same fabric
        // scope that is for this session
        if self
            .failsafe
            .disarm(cmd_req.trans.session.get_session_mode())
            .is_err()
        {
            status = CommissioningError::ErrInvalidAuth as u8;
        }

        let cmd_data = |t: &mut TLVWriter| {
            t.u8(TagType::Context(0), status)?;
            t.utf8(TagType::Context(1), b"")
        };

        let resp =
            ib::InvResp::cmd_new(0, ID, Commands::CommissioningCompleteResp as u16, &cmd_data);
        let _ = resp.to_tlv(cmd_req.resp, TagType::Anonymous);
        cmd_req.trans.complete();
        Ok(())
    }
}
