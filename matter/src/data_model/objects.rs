use crate::{
    error::*,
    interaction_model::{command::CommandReq, core::IMStatusCode, read::attr_path},
    tlv_common::TagType,
    tlv_writer::{TLVWriter, ToTLV},
};
use log::error;
use std::{any::Any, fmt};

/* This file needs some major revamp.
 * - instead of allocating all over the heap, we should use some kind of slab/block allocator
 * - instead of arrays, can use linked-lists to conserve space and avoid the internal fragmentation
 */
pub const ENDPTS_PER_ACC: usize = 3;
pub const CLUSTERS_PER_ENDPT: usize = 4;
pub const ATTRS_PER_CLUSTER: usize = 4;
pub const CMDS_PER_CLUSTER: usize = 8;

#[derive(Debug)]
pub enum AttrValue {
    Int8(i8),
    Int64(i64),
    Uint16(u16),
    Bool(bool),
}

impl ToTLV for AttrValue {
    fn to_tlv(&self, tw: &mut TLVWriter, tag_type: TagType) -> Result<(), Error> {
        match self {
            AttrValue::Bool(v) => tw.put_bool(tag_type, *v),
            AttrValue::Uint16(v) => tw.put_u16(tag_type, *v),
            _ => {
                error!("Not yet supported");
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub struct Attribute {
    id: u16,
    value: AttrValue,
}

impl Default for Attribute {
    fn default() -> Attribute {
        Attribute {
            id: 0,
            value: AttrValue::Bool(true),
        }
    }
}

impl Attribute {
    pub fn new(id: u16, val: AttrValue) -> Result<Box<Attribute>, Error> {
        let mut a = Box::new(Attribute::default());
        a.id = id;
        a.value = val;
        Ok(a)
    }
}

impl std::fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.id, self.value)
    }
}

pub type CommandCb = fn(&mut Cluster, cmd_req: &mut CommandReq) -> Result<(), IMStatusCode>;

pub struct Command {
    id: u16,
    cb: CommandCb,
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id:{}", self.id)
    }
}

impl Command {
    pub fn new(id: u16, cb: CommandCb) -> Result<Box<Command>, Error> {
        Ok(Box::new(Command { id, cb }))
    }
}

#[derive(Default)]
pub struct Cluster {
    id: u32,
    attributes: [Option<Box<Attribute>>; ATTRS_PER_CLUSTER],
    commands: [Option<Box<Command>>; CMDS_PER_CLUSTER],
    data: Option<Box<dyn Any>>,
}

impl Cluster {
    pub fn new(id: u32) -> Result<Box<Cluster>, Error> {
        let mut a = Box::new(Cluster::default());
        a.id = id;
        Ok(a)
    }

    pub fn set_data(&mut self, data: Box<dyn Any>) {
        self.data = Some(data);
    }

    pub fn get_data<T: Any>(&mut self) -> Option<&mut T> {
        self.data.as_mut()?.downcast_mut::<T>()
    }

    pub fn clear_data(&mut self) {
        self.data = None;
    }

    pub fn add_attribute(&mut self, attr: Box<Attribute>) -> Result<(), Error> {
        for c in self.attributes.iter_mut() {
            if c.is_none() {
                *c = Some(attr);
                return Ok(());
            }
        }
        Err(Error::NoSpace)
    }

    pub fn add_command(&mut self, command: Box<Command>) -> Result<(), Error> {
        for c in self.commands.iter_mut() {
            if c.is_none() {
                *c = Some(command);
                return Ok(());
            }
        }
        Err(Error::NoSpace)
    }

    pub fn handle_command(&mut self, cmd_req: &mut CommandReq) -> Result<(), IMStatusCode> {
        let cmd = self
            .commands
            .iter()
            .flatten()
            .find(|x| x.id == cmd_req.command)
            .ok_or(IMStatusCode::UnsupportedCommand)?;
        (cmd.cb)(self, cmd_req)
    }

    fn get_attribute_index(&mut self, attr_id: u16) -> Option<usize> {
        self.attributes
            .iter()
            .position(|x| x.as_ref().map_or(false, |c| c.id == attr_id))
    }

    pub fn handle_attrs_read(
        &mut self,
        endpoint: u16,
        attribute: Option<u16>,
        tw: &mut TLVWriter,
    ) -> Result<(), IMStatusCode> {
        let cluster_id = self.id;
        let attributes = self.get_wildcard_attribute(attribute)?;

        for a in attributes.iter_mut().flatten() {
            let attr_path = attr_path::Ib::new(endpoint, cluster_id, a.id);
            // For now, putting everything in here
            let _ = tw.put_start_struct(TagType::Anonymous);
            let _ = tw.put_start_struct(TagType::Context(1));
            let _ = tw.put_object(TagType::Context(1), &attr_path);
            // We will have to also support custom data types for encoding
            let _ = tw.put_object(TagType::Context(2), &a.value);
            let _ = tw.put_end_container();
            let _ = tw.put_end_container();
        }
        Ok(())
    }

    // Returns a slice of attribute, with either a single attribute or all (wildcard)
    pub fn get_wildcard_attribute(
        &mut self,
        attribute: Option<u16>,
    ) -> Result<&mut [Option<Box<Attribute>>], IMStatusCode> {
        let attributes = if let Some(a) = attribute {
            if let Some(i) = self.get_attribute_index(a) {
                &mut self.attributes[i..i + 1]
            } else {
                return Err(IMStatusCode::UnsupportedAttribute);
            }
        } else {
            &mut self.attributes[..]
        };
        Ok(attributes)
    }
}

impl std::fmt::Display for Cluster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id:{}, ", self.id)?;
        write!(f, "attrs[")?;
        let mut comma = "";
        for element in self.attributes.iter() {
            if let Some(e) = element {
                write!(f, "{} {}", comma, e)?;
            }
            comma = ",";
        }
        write!(f, " ], ")?;
        write!(f, "cmds[")?;
        let mut comma = "";
        for element in self.commands.iter() {
            if let Some(e) = element {
                write!(f, "{} {}", comma, e)?;
            }
            comma = ",";
        }
        write!(f, " ]")
    }
}

#[derive(Default)]
pub struct Endpoint {
    clusters: [Option<Box<Cluster>>; CLUSTERS_PER_ENDPT],
}

impl Endpoint {
    pub fn new() -> Result<Box<Endpoint>, Error> {
        Ok(Box::new(Endpoint::default()))
    }

    pub fn add_cluster(&mut self, cluster: Box<Cluster>) -> Result<(), Error> {
        for c in self.clusters.iter_mut() {
            if c.is_none() {
                *c = Some(cluster);
                return Ok(());
            }
        }
        Err(Error::NoSpace)
    }

    fn get_cluster_index(&mut self, cluster_id: u32) -> Option<usize> {
        self.clusters
            .iter()
            .position(|x| x.as_ref().map_or(false, |c| c.id == cluster_id))
    }

    pub fn get_cluster(&mut self, cluster_id: u32) -> Result<&mut Box<Cluster>, Error> {
        let index = self
            .get_cluster_index(cluster_id)
            .ok_or(Error::ClusterNotFound)?;
        Ok(self.clusters[index].as_mut().unwrap())
    }

    // Returns a slice of clusters, with either a single cluster or all (wildcard)
    pub fn get_wildcard_clusters(
        &mut self,
        cluster: Option<u32>,
    ) -> Result<&mut [Option<Box<Cluster>>], IMStatusCode> {
        let clusters = if let Some(c) = cluster {
            if let Some(i) = self.get_cluster_index(c) {
                &mut self.clusters[i..i + 1]
            } else {
                return Err(IMStatusCode::UnsupportedCluster);
            }
        } else {
            &mut self.clusters[..]
        };
        Ok(clusters)
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "clusters:[")?;
        let mut comma = "";
        for element in self.clusters.iter().flatten() {
            write!(f, "{} {{ {} }}", comma, element)?;
            comma = ", ";
        }
        write!(f, "]")
    }
}

#[derive(Default)]
pub struct Node {
    endpoints: [Option<Box<Endpoint>>; ENDPTS_PER_ACC],
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "node:")?;
        for (i, element) in self.endpoints.iter().enumerate() {
            if let Some(e) = element {
                writeln!(f, "endpoint {}: {}", i, e)?;
            }
        }
        write!(f, "")
    }
}

impl Node {
    pub fn new() -> Result<Box<Node>, Error> {
        let node = Box::new(Node::default());
        Ok(node)
    }

    pub fn add_endpoint(&mut self) -> Result<u32, Error> {
        let index = self
            .endpoints
            .iter()
            .position(|x| x.is_none())
            .ok_or(Error::NoSpace)?;
        self.endpoints[index] = Some(Endpoint::new()?);
        Ok(index as u32)
    }

    pub fn get_endpoint(&mut self, endpoint_id: u32) -> Result<&mut Box<Endpoint>, Error> {
        if (endpoint_id as usize) < ENDPTS_PER_ACC {
            let endpoint = self.endpoints[endpoint_id as usize]
                .as_mut()
                .ok_or(Error::EndpointNotFound)?;
            Ok(endpoint)
        } else {
            Err(Error::EndpointNotFound)
        }
    }

    pub fn add_cluster(&mut self, endpoint_id: u32, cluster: Box<Cluster>) -> Result<(), Error> {
        let endpoint_id = endpoint_id as usize;
        if endpoint_id < ENDPTS_PER_ACC {
            self.endpoints[endpoint_id]
                .as_mut()
                .ok_or(Error::NoEndpoint)?
                .add_cluster(cluster)
        } else {
            Err(Error::Invalid)
        }
    }

    // Returns a slice of endpoints, with either a single endpoint or all (wildcard)
    pub fn get_wildcard_endpoints(
        &mut self,
        endpoint: Option<u16>,
    ) -> Result<(&mut [Option<Box<Endpoint>>], usize), IMStatusCode> {
        let endpoints = if let Some(e) = endpoint {
            let e = e as usize;
            if self.endpoints[e].is_none() {
                return Err(IMStatusCode::UnsupportedEndpoint);
            }
            (&mut self.endpoints[e..e + 1], e)
        } else {
            (&mut self.endpoints[..], 0)
        };
        Ok(endpoints)
    }

    pub fn for_cluster_path<F>(
        &mut self,
        endpoint: Option<u16>,
        cluster: Option<u32>,
        mut f: F,
    ) -> Result<(), IMStatusCode>
    where
        F: FnMut(u16, &mut Box<Cluster>) -> Result<(), IMStatusCode>,
    {
        let (endpoints, mut endpoint_id) = self.get_wildcard_endpoints(endpoint)?;
        for e in endpoints.iter_mut().flatten() {
            let clusters = e.get_wildcard_clusters(cluster)?;
            for c in clusters.iter_mut().flatten() {
                f(endpoint_id as u16, c)?;
            }
            endpoint_id += 1;
        }
        Ok(())
    }
}
