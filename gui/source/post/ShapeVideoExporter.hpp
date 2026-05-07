#pragma once
#include "solver/BowResult.hpp"
#include <QObject>
#include <QString>

class QWidget;
class ShapePlot;

// Renders the bow simulation to a sequence of PNG frames and (when ffmpeg
// is available on PATH) encodes them into a single video file. The output
// is composed of three phases concatenated in order:
//
//   1. Static (pulling) phase — frames sampled from `data.statics->states`
//   2. Full-draw hold         — last static frame repeated for `hold_seconds`
//   3. Dynamic (release) phase — frames from `data.dynamics->states`
//
// All three phases are rendered through a single off-screen ShapePlot built
// from a synthetic States object that concatenates the static and dynamic
// geometric data. This guarantees that:
//   • the final pulling frame and the first dynamic frame have identical
//     bow geometry (same arrays — no boundary jump);
//   • axes (zoom/pan) stay constant across the whole video, computed once
//     over the union of both phases;
//   • the gray background "ghost" states from the pulling phase remain
//     visible during the dynamic phase as well.
class ShapeVideoExporter: public QObject {
    Q_OBJECT
public:
    // `parent_widget` parents any modal dialogs and owns the off-screen plot.
    // `size_reference` is the visible dynamic ShapePlot; its size at the
    // moment run() is invoked is used as the output frame size, so the video
    // matches what the user currently sees. `data` provides the bow Common
    // info plus the Static and Dynamic States.
    //
    // `hold_seconds`   : how long to freeze on the final pulling frame.
    // `static_seconds` : how long the pulling phase should play. The static
    //                    simulation typically produces only a few states
    //                    (driven by the user's `n_draw_steps`); we step-
    //                    stretch them across `static_seconds * fps` frames
    //                    so the motion reads as smooth instead of staccato.
    //                    Set to 0 to emit one output frame per source state.
    ShapeVideoExporter(QWidget* parent_widget, ShapePlot* size_reference,
                       const BowResult& data,
                       double hold_seconds = 2.0, double static_seconds = 3.0);

    ~ShapeVideoExporter();

    // Show file dialogs and run the export. Returns true on success.
    bool run();

private:
    QWidget* parent_widget;
    ShapePlot* size_reference;
    const BowResult& data;
    double hold_seconds;
    double static_seconds;

    // Synthetic States combining the static and dynamic phases. Held by
    // value so it outlives the off-screen ShapePlot, which keeps a const
    // reference to it.
    States combined_states;
    int n_static = 0;
    int n_dynamic = 0;

    // Off-screen ShapePlot that drives every video frame. Built lazily on
    // the first run() call and reused on subsequent calls.
    ShapePlot* video_plot = nullptr;
};
