//! pat-nzgdu.109: GraphEdit connection list + typed-port validity, and
//! VideoStreamPlayer playback state.
//!
//! The bead's acceptance, in one test:
//!
//! 1. A `GraphEdit` with two `GraphNode`s and `connect_node(from, 0, to, 0)`
//!    reports the connection in `get_connection_list()`.
//! 2. Attempting an invalid *type* connection is rejected.
//! 3. A `VideoStreamPlayer` with a loaded stream reports `is_playing() == true`
//!    after `play()`.

use gdresource::image::{Image, ImageFormat};
use gdscene::graph_edit::{GraphEdit, GraphEditError, GraphFrame, GraphNode};
use gdscene::video_stream::{VideoStream, VideoStreamPlayer};

/// Port type ids, as a game would define them (Godot port types are ints).
const TYPE_SCALAR: i32 = 0;
const TYPE_VECTOR: i32 = 1;

fn one_frame_stream(frames: usize, fps: f64) -> VideoStream {
    let images: Vec<Image> = (0..frames)
        .map(|_| Image::create(1, 1, ImageFormat::Rgba8))
        .collect();
    VideoStream::from_frames(images, fps).expect("positive fps builds a stream")
}

#[test]
fn graph_edit_connections_and_video() {
    // ---- 1. Two GraphNodes, connect_node, get_connection_list ------------
    let mut graph = GraphEdit::new();

    let mut source = GraphNode::new("Source");
    source.set_output_slot(0, TYPE_SCALAR);
    let mut sink = GraphNode::new("Sink");
    sink.set_input_slot(0, TYPE_SCALAR);

    graph.add_node("source", source);
    graph.add_node("sink", sink);
    assert_eq!(
        graph.node_names(),
        &["source".to_string(), "sink".to_string()][..]
    );
    assert!(
        graph.get_connection_list().is_empty(),
        "a fresh graph has no connections"
    );

    graph
        .connect_node("source", 0, "sink", 0)
        .expect("matching port types should connect");

    let list = graph.get_connection_list();
    assert_eq!(
        list.len(),
        1,
        "the connection should be listed exactly once"
    );
    assert_eq!(list[0].from_node, "source");
    assert_eq!(list[0].from_port, 0);
    assert_eq!(list[0].to_node, "sink");
    assert_eq!(list[0].to_port, 0);
    assert!(graph.is_node_connected("source", 0, "sink", 0));

    // ---- 2. An invalid type connection is rejected ------------------------
    let mut typed = GraphNode::new("Vector");
    typed.set_output_slot(0, TYPE_VECTOR);
    graph.add_node("vector", typed);

    // TYPE_VECTOR output -> TYPE_SCALAR input: not whitelisted, so rejected.
    assert_eq!(
        graph.connect_node("vector", 0, "sink", 0),
        Err(GraphEditError::IncompatibleTypes {
            from_type: TYPE_VECTOR,
            to_type: TYPE_SCALAR,
        }),
        "mismatched port types must be rejected"
    );
    assert_eq!(
        graph.get_connection_list().len(),
        1,
        "a rejected connection must not be recorded"
    );
    assert!(!graph.is_node_connected("vector", 0, "sink", 0));

    // Whitelisting the ordered pair makes exactly that direction legal.
    graph.add_valid_connection_type(TYPE_VECTOR, TYPE_SCALAR);
    assert!(graph.is_valid_connection_type(TYPE_VECTOR, TYPE_SCALAR));
    assert!(
        !graph.is_valid_connection_type(TYPE_SCALAR, TYPE_VECTOR),
        "the whitelist is directional"
    );
    graph
        .connect_node("vector", 0, "sink", 0)
        .expect("whitelisted type pair should connect");
    assert_eq!(graph.get_connection_list().len(), 2);

    // Disconnecting leaves the other edge intact.
    graph
        .disconnect_node("source", 0, "sink", 0)
        .expect("connected edge disconnects");
    assert_eq!(graph.get_connection_list().len(), 1);
    assert!(graph.is_node_connected("vector", 0, "sink", 0));

    // Removing a node drops the connections that touch it.
    assert!(graph.remove_node("sink"));
    assert!(
        graph.get_connection_list().is_empty(),
        "connections to a removed node must not linger"
    );

    // GraphFrame groups elements by name.
    let mut frame = GraphFrame::new("Group");
    frame.attach("source");
    graph.add_frame("frame", frame);
    assert!(graph.get_frame("frame").unwrap().is_attached("source"));

    // ---- 3. VideoStreamPlayer reports is_playing after play() ------------
    let mut player = VideoStreamPlayer::default();
    assert!(!player.is_playing(), "a fresh player is not playing");

    let stream = one_frame_stream(4, 2.0); // 4 frames @ 2fps = 2.0s
    player.set_stream(stream);
    assert!((player.length_secs - 2.0).abs() < 1e-9);

    player.play();
    assert!(
        player.is_playing(),
        "is_playing() must be true after play() with a loaded stream"
    );

    // paused suppresses is_playing without rewinding, as in Godot.
    player.set_paused(true);
    assert!(!player.is_playing(), "paused player reports not playing");
    player.set_paused(false);
    assert!(player.is_playing());

    // Advancing past the end stops a non-looping stream.
    player.advance(5.0);
    assert!(
        !player.is_playing(),
        "playback stops at the end of the stream"
    );

    player.play();
    player.stop();
    assert!(!player.is_playing(), "stop() halts playback");
    assert_eq!(player.position_secs, 0.0, "stop() rewinds to the start");
}
