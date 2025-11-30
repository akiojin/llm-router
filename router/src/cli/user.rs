//! User management CLI subcommands
//!
//! Provides commands for user list, add, and delete operations.

use clap::{Parser, Subcommand};

/// User management commands
#[derive(Subcommand, Debug)]
pub enum UserCommand {
    /// List all users
    List,
    /// Add a new user
    Add(AddUser),
    /// Delete a user
    Delete(DeleteUser),
}

/// Arguments for adding a new user
#[derive(Parser, Debug)]
pub struct AddUser {
    /// Username for the new user
    pub username: String,
    /// Password (min 8 characters)
    #[arg(short, long)]
    pub password: String,
}

/// Arguments for deleting a user
#[derive(Parser, Debug)]
pub struct DeleteUser {
    /// Username to delete
    pub username: String,
}
