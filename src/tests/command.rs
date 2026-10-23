//! Dispatch tests for the command system: player input must resolve to the
//! single path whose arguments parse, and bad input must never panic.

use std::time::Duration;

use bevy_app::App;
use bevy_ecs::message::Messages;
use chunkedge_ident::ident;
use chunkedge_registry::{BiomeRegistry, DimensionTypeRegistry};
use chunkedge_server::keepalive::KeepaliveSettings;
use chunkedge_server::{ChunkLayer, EntityLayer, Server, ServerSettings};

use crate::command::graph::CommandGraphBuilder;
use crate::command::handler::CommandResultMessage;
use crate::command::parsers::{CommandArg, CommandArgParseError, GreedyString};
use crate::command::{AddCommand, Command};
use crate::command_macros::Command;
use crate::protocol::packets::play::{ChatCommandC2s, SystemChatS2c};
use crate::testing::{self, MockClientHelper, create_mock_client};

/// Mirrors the ambiguous variants of `TestCommand` in `examples/command.rs`:
/// `/test a 1 b 2 c 3` matches `A` through its literal path and `C`/`D`
/// through their greedy string, while `A`'s all-arguments path fails on `b`.
#[derive(Command, Debug, Clone, PartialEq)]
#[paths("test", "t")]
enum TestCommand {
    #[paths("a {a} b {b} c {c}", "{a} {b} {c}")]
    A { a: String, b: i32, c: f32 },
    #[paths = "a {a} b {b?}"]
    C { a: String, b: Option<GreedyString> },
    #[paths = "a {a} b {b}"]
    D { a: String, b: GreedyString },
}

/// A command whose executable rejects its input even though the dispatcher
/// accepted it, to exercise the error path of the handler.
#[derive(Debug, Clone, PartialEq)]
struct RejectingCommand;

impl Command for RejectingCommand {
    fn assemble_graph(graph: &mut CommandGraphBuilder<Self>) {
        graph
            .root()
            .literal("reject")
            .argument("value")
            .with_parser::<GreedyString>()
            .with_executable(|input| {
                i32::parse_arg(input)?;
                Ok(RejectingCommand)
            });
    }
}

struct CommandScenario {
    app: App,
    helper: MockClientHelper,
}

impl CommandScenario {
    fn new() -> Self {
        let mut app = App::new();

        app.insert_resource(KeepaliveSettings {
            period: Duration::MAX,
        })
        .insert_resource(ServerSettings {
            compression_threshold: Default::default(),
            ..Default::default()
        });

        testing::add_plugins(&mut app);
        app.add_command::<TestCommand>()
            .add_command::<RejectingCommand>();

        app.update(); // Initialize plugins and build the command graph.

        let chunk_layer = ChunkLayer::new(
            ident!("overworld"),
            app.world().resource::<DimensionTypeRegistry>(),
            app.world().resource::<BiomeRegistry>(),
            app.world().resource::<Server>(),
        );
        let entity_layer = EntityLayer::new(app.world().resource::<Server>());
        let layer = app.world_mut().spawn((chunk_layer, entity_layer)).id();

        let (mut client, helper) = create_mock_client("test");
        client.layer.0 = layer;
        client.visible_chunk_layer.0 = layer;
        client.visible_entity_layers.0.insert(layer);
        app.world_mut().spawn(client);

        app.update();

        Self { app, helper }
    }

    /// Sends `command` as the client and returns every result it produced.
    fn dispatch<T: Command + Clone + Send + Sync + 'static>(&mut self, command: &str) -> Vec<T> {
        self.helper.clear_received();
        self.helper.send(&ChatCommandC2s {
            command: command.into(),
        });
        self.app.update();

        let messages = self
            .app
            .world()
            .resource::<Messages<CommandResultMessage<T>>>();
        messages
            .get_cursor()
            .read(messages)
            .map(|message| message.result.clone())
            .collect()
    }
}

#[test]
fn literal_path_wins_over_greedy_alternatives() {
    let mut scenario = CommandScenario::new();

    let results = scenario.dispatch::<TestCommand>("test a 1 b 2 c 3");

    assert_eq!(
        results,
        [TestCommand::A {
            a: "1".to_owned(),
            b: 2,
            c: 3.0,
        }]
    );
}

#[test]
fn all_argument_path_is_used_when_literals_do_not_match() {
    let mut scenario = CommandScenario::new();

    let results = scenario.dispatch::<TestCommand>("test x 1 2.5");

    assert_eq!(
        results,
        [TestCommand::A {
            a: "x".to_owned(),
            b: 1,
            c: 2.5,
        }]
    );
}

#[test]
fn falls_back_to_argument_path_when_literal_path_fails() {
    let mut scenario = CommandScenario::new();

    let results = scenario.dispatch::<TestCommand>("test a 1 2");

    assert_eq!(
        results,
        [TestCommand::A {
            a: "a".to_owned(),
            b: 1,
            c: 2.0,
        }]
    );
}

#[test]
fn redirected_alias_resolves_like_the_command() {
    let mut scenario = CommandScenario::new();

    let results = scenario.dispatch::<TestCommand>("t a 1 b hello world");

    assert_eq!(
        results,
        [TestCommand::C {
            a: "1".to_owned(),
            b: Some(GreedyString("hello world".to_owned())),
        }]
    );
}

#[test]
fn malformed_arguments_are_rejected_without_panicking() {
    let mut scenario = CommandScenario::new();

    for command in ["test x y z", "test 1 2 c", "test a", "test", "t x 1 y"] {
        let results = scenario.dispatch::<TestCommand>(command);
        assert_eq!(results, [], "`/{command}` should not execute");
    }
}

#[test]
fn executable_parse_error_is_reported_to_the_player() {
    let mut scenario = CommandScenario::new();

    let results = scenario.dispatch::<RejectingCommand>("reject not-a-number");

    assert_eq!(results, []);
    let received = scenario.helper.collect_received();
    received.assert_count::<SystemChatS2c>(1);
    let chat = received
        .first::<SystemChatS2c>()
        .chat
        .text
        .to_legacy_lossy();
    assert!(
        chat.contains(
            &CommandArgParseError::InvalidArgument {
                expected: "integer".to_owned(),
                got: "not-a-number".to_owned(),
            }
            .to_string()
        ),
        "unexpected error message: {chat}"
    );
}
