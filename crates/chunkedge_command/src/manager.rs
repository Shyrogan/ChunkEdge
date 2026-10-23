use std::collections::{HashMap, HashSet};

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{
    Added, Changed, Commands, DetectChanges, IntoScheduleConfigs, Message, MessageReader,
    MessageWriter, Mut, Or, Query, Res,
};
use chunkedge_server::EventLoopPreUpdate;
use chunkedge_server::client::{Client, SpawnClientsSet};
use chunkedge_server::event_loop::PacketMessage;
use chunkedge_server::protocol::WritePacket;
use chunkedge_server::protocol::packets::play::commands_s2c::NodeData;
use chunkedge_server::protocol::packets::play::{
    ChatCommandC2s, ChatCommandSignedC2s, CommandsS2c,
};
use petgraph::graph::NodeIndex;
use petgraph::prelude::EdgeRef;
use petgraph::{Direction, Graph};
use tracing::{debug, error, info, trace, warn};

use crate::graph::{CommandEdgeType, CommandGraph};
use crate::parsers::ParseInput;
use crate::scopes::{CommandScopePlugin, CommandScopes};
use crate::{CommandRegistry, CommandScopeRegistry, CommandSystemSet, ModifierValue};

pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(CommandScopePlugin)
            .add_message::<CommandExecutionMessage>()
            .add_message::<CommandProcessedMessage>()
            .add_systems(PreUpdate, insert_scope_component.after(SpawnClientsSet))
            .add_systems(
                EventLoopPreUpdate,
                (
                    update_command_tree,
                    command_tree_update_with_client,
                    read_incoming_packets.before(CommandSystemSet),
                    parse_incoming_commands.in_set(CommandSystemSet),
                ),
            );

        let graph: CommandGraph = CommandGraph::new();
        let modifiers = HashMap::new();
        let parsers = HashMap::new();
        let executables = HashSet::new();

        app.insert_resource(CommandRegistry {
            graph,
            parsers,
            modifiers,
            executables,
        });
    }
}

/// This message is sent when a command is sent (you can send this with any
/// entity)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Message)]
pub struct CommandExecutionMessage {
    /// the command that was executed eg. "teleport @p 0 ~ 0"
    pub command: String,
    /// usually the Client entity but it could be a command block or something
    /// (whatever the library user wants)
    pub executor: Entity,
}

/// This will only be sent if the command was successfully parsed and an
/// executable was found
#[derive(Debug, Clone, PartialEq, Eq, Message)]
pub struct CommandProcessedMessage {
    /// the command that was executed eg. "teleport @p 0 ~ 0"
    pub command: String,
    /// usually the Client entity but it could be a command block or something
    /// (whatever the library user wants)
    pub executor: Entity,
    /// the modifiers that were applied to the command
    pub modifiers: HashMap<ModifierValue, ModifierValue>,
    /// the node that was executed
    pub node: NodeIndex,
}

fn insert_scope_component(mut clients: Query<Entity, Added<Client>>, mut commands: Commands) {
    for client in &mut clients {
        commands.entity(client).insert(CommandScopes::new());
    }
}

fn read_incoming_packets(
    mut packets: MessageReader<PacketMessage>,
    mut message_writer: MessageWriter<CommandExecutionMessage>,
) {
    for packet in packets.read() {
        let client = packet.client;

        if let Some(unsigned) = packet.decode::<ChatCommandC2s>() {
            message_writer.write(CommandExecutionMessage {
                command: unsigned.command.to_string(),
                executor: client,
            });
        } else if let Some(signed) = packet.decode::<ChatCommandSignedC2s>() {
            // TODO: verify command signatures.
            // As per this gist: https://gist.github.com/kennytv/ed783dd244ca0321bbd882c347892874
            // It looks like the client only sends the signed version if a command requires
            // it (in vanilla thats /say for example).
            message_writer.write(CommandExecutionMessage {
                command: signed.command.to_string(),
                executor: client,
            });
        }
    }
}

#[allow(clippy::type_complexity)]
fn command_tree_update_with_client(
    command_registry: Res<CommandRegistry>,
    scope_registry: Res<CommandScopeRegistry>,
    mut updated_clients: Query<
        (&mut Client, &CommandScopes),
        Or<(Added<Client>, Changed<CommandScopes>)>,
    >,
) {
    update_client_command_tree(
        &command_registry,
        scope_registry,
        &mut updated_clients.iter_mut().collect(),
    );
}

fn update_command_tree(
    command_registry: Res<CommandRegistry>,
    scope_registry: Res<CommandScopeRegistry>,
    mut clients: Query<(&mut Client, &CommandScopes)>,
) {
    if command_registry.is_changed() {
        update_client_command_tree(
            &command_registry,
            scope_registry,
            &mut clients.iter_mut().collect(),
        );
    }
}

fn update_client_command_tree(
    command_registry: &Res<CommandRegistry>,
    scope_registry: Res<CommandScopeRegistry>,
    updated_clients: &mut Vec<(Mut<Client>, &CommandScopes)>,
) {
    for (client, client_scopes) in updated_clients {
        let time = std::time::Instant::now();

        let old_graph = &command_registry.graph;
        let mut new_graph = Graph::new();

        // collect a new graph into only nodes that are allowed to be executed
        let root = old_graph.root;

        let mut to_visit = vec![(None, root)];
        let mut already_visited = HashSet::new(); // prevent recursion
        let mut old_to_new = HashMap::new();
        let mut new_root = None;

        while let Some((parent, node)) = to_visit.pop() {
            if already_visited.contains(&(parent.map(|(node_id, _)| node_id), node)) {
                continue;
            }
            already_visited.insert((parent.map(|(node_id, _)| node_id), node));
            let node_scopes = &old_graph.graph[node].scopes;
            if !node_scopes.is_empty() {
                let mut has_scope = false;
                for scope in node_scopes {
                    if scope_registry.any_grants(
                        &client_scopes.0.iter().map(|scope| scope.as_str()).collect(),
                        scope,
                    ) {
                        has_scope = true;
                        break;
                    }
                }
                if !has_scope {
                    continue;
                }
            }

            let new_node = *old_to_new
                .entry(node)
                .or_insert_with(|| new_graph.add_node(old_graph.graph[node].clone()));

            for neighbor in old_graph.graph.edges_directed(node, Direction::Outgoing) {
                to_visit.push((Some((new_node, neighbor.weight())), neighbor.target()));
            }

            if let Some(parent) = parent {
                new_graph.add_edge(parent.0, new_node, *parent.1);
            } else {
                new_root = Some(new_node);
            }
        }

        match new_root {
            Some(new_root) => {
                let command_graph = CommandGraph {
                    graph: new_graph,
                    root: new_root,
                };
                let packet: CommandsS2c = command_graph.into();

                client.write_packet(&packet);
            }
            None => {
                warn!(
                    "Client has no permissions to execute any commands so we sent them nothing. \
                     It is generally a bad idea to scope the root node of the command graph as it \
                     can cause undefined behavior. For example, if the player has permission to \
                     execute a command before you change the scope of the root node, the packet \
                     will not be sent to the client and so the client will still think they can \
                     execute the command."
                )
            }
        }

        debug!("command tree update took {:?}", time.elapsed());
    }
}

fn parse_incoming_commands(
    mut message_reader: MessageReader<CommandExecutionMessage>,
    mut message_writer: MessageWriter<CommandProcessedMessage>,
    command_registry: Res<CommandRegistry>,
    scope_registry: Res<CommandScopeRegistry>,
    entity_scopes: Query<&CommandScopes>,
) {
    for command_message in message_reader.read() {
        let executor = command_message.executor;
        let default_scopes = CommandScopes::new();
        let executor_scopes = entity_scopes
            .get(executor)
            .unwrap_or(&default_scopes)
            .0
            .iter()
            .map(String::as_str)
            .collect();
        let dispatcher = Dispatcher {
            registry: &command_registry,
            scope_registry: &scope_registry,
            executor_scopes,
        };

        let Some(matched) = dispatcher.resolve(
            command_registry.graph.root,
            ParseInput::new(&command_message.command),
            false,
            Vec::new(),
            Vec::new(),
        ) else {
            debug!("No command matched: /{}", command_message.command);
            continue;
        };

        let mut modifiers = HashMap::new();
        for (node, modifier) in matched.modifiers {
            command_registry.modifiers[&node](modifier, &mut modifiers);
        }

        trace!("executing node: {:?}", matched.node);
        info!(
            "Command dispatched: /{} (debug logs for more data)",
            command_message.command
        );
        debug!("Command modifiers: {:?}", modifiers);
        message_writer.write(CommandProcessedMessage {
            command: matched.args.join(" "),
            executor,
            modifiers,
            node: matched.node,
        });
    }
}

/// The executable node a command resolved to.
struct CommandMatch {
    node: NodeIndex,
    /// The raw text of each argument on the path, re-parsed by the executable.
    args: Vec<String>,
    /// The modifier nodes on the path with the text they matched.
    modifiers: Vec<(NodeIndex, String)>,
}

struct Dispatcher<'a> {
    registry: &'a CommandRegistry,
    scope_registry: &'a CommandScopeRegistry,
    executor_scopes: Vec<&'a str>,
}

impl Dispatcher<'_> {
    /// Matches `node` against the start of `input` and resolves the rest of
    /// the input through its children, returning the first path that ends on
    /// an executable node with all input consumed.
    ///
    /// Only one path is ever selected, and the returned arguments are the ones
    /// read along that path. Like brigadier, literal children are tried before
    /// argument children, so `/test a 1` prefers a literal `a` over an argument
    /// that reads `a`. A branch that fails to parse falls back to the next
    /// child.
    fn resolve(
        &self,
        node: NodeIndex,
        mut input: ParseInput,
        via_redirect: bool,
        mut args: Vec<String>,
        mut modifiers: Vec<(NodeIndex, String)>,
    ) -> Option<CommandMatch> {
        if !self.can_access(node) {
            return None;
        }

        let graph = &self.registry.graph.graph;

        if via_redirect {
            // Arguments read before a redirect belong to the redirecting node,
            // not to the command we land in.
            args.clear();
        } else {
            input.skip_whitespace();
            let matched = match &graph[node].data {
                NodeData::Root => String::new(),
                NodeData::Literal { name } => {
                    // The literal must be a whole word.
                    if !input.match_next(name) || !(input.match_next(" ") || input.is_done()) {
                        return None;
                    }
                    String::new()
                }
                NodeData::Argument { .. } => {
                    let parser = self.registry.parsers.get(&node)?;
                    let before = input.clone().into_inner();
                    if !parser(&mut input) {
                        return None;
                    }
                    let Some(arg) = before.get(..before.len().wrapping_sub(input.len())) else {
                        error!(
                            "Parser replaced input with another string. This is not allowed. \
                             Attempting to parse: {}",
                            input.into_inner()
                        );
                        return None;
                    };
                    args.push(arg.to_owned());
                    arg.to_owned()
                }
            };
            if self.registry.modifiers.contains_key(&node) {
                modifiers.push((node, matched));
            }
        }

        input.skip_whitespace();
        if input.is_done() && self.registry.executables.contains(&node) {
            return Some(CommandMatch {
                node,
                args,
                modifiers,
            });
        }

        let mut edges: Vec<_> = graph.edges_directed(node, Direction::Outgoing).collect();
        // petgraph lists the newest edge first: restore registration order, then
        // move literals ahead of arguments (the sort is stable).
        edges.reverse();
        edges.sort_by_key(|edge| !matches!(graph[edge.target()].data, NodeData::Literal { .. }));

        edges.into_iter().find_map(|edge| {
            self.resolve(
                edge.target(),
                input.clone(),
                matches!(edge.weight(), CommandEdgeType::Redirect),
                args.clone(),
                modifiers.clone(),
            )
        })
    }

    /// Nodes without scopes are open to everyone.
    fn can_access(&self, node: NodeIndex) -> bool {
        let node_scopes = &self.registry.graph.graph[node].scopes;
        node_scopes.is_empty()
            || node_scopes
                .iter()
                .any(|scope| self.scope_registry.any_grants(&self.executor_scopes, scope))
    }
}
