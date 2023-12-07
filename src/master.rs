use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SlaveConnectionDescriptor {
    address: String,
    port: String
} // this is the inactive version for configuration, there is / will be a copy of this that is a active connection