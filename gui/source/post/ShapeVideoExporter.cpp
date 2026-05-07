#include "ShapeVideoExporter.hpp"
#include "ShapePlot.hpp"
#include "pre/models/units/UnitSystem.hpp"

#include <QApplication>
#include <QCoreApplication>
#include <QDesktopServices>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QInputDialog>
#include <QMessageBox>
#include <QOperatingSystemVersion>
#include <QPixmap>
#include <QProcess>
#include <QProgressDialog>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QUrl>

#include <algorithm>
#include <cmath>

namespace {

// Platform-specific install instructions for ffmpeg. Returned as plain text
// suitable for a QMessageBox detail block, plus a clickable hint URL.
struct FfmpegInstallHelp {
    QString instructions;
    QString help_url;
};

FfmpegInstallHelp ffmpeg_install_help() {
    FfmpegInstallHelp h;
    h.help_url = "https://ffmpeg.org/download.html";

#if defined(Q_OS_WIN)
    h.instructions =
        QObject::tr(
            "Install one of the following on Windows:\n"
            "\n"
            "  • winget install --id Gyan.FFmpeg\n"
            "  • choco install ffmpeg-full\n"
            "  • scoop install ffmpeg\n"
            "\n"
            "Or download a static build from %1 and add the bin directory to PATH.")
        .arg(h.help_url);
#elif defined(Q_OS_MAC)
    h.instructions =
        QObject::tr(
            "Install on macOS via Homebrew:\n"
            "\n"
            "  brew install ffmpeg\n"
            "\n"
            "Or via MacPorts:\n"
            "\n"
            "  sudo port install ffmpeg\n"
            "\n"
            "More info: %1")
        .arg(h.help_url);
#else
    h.instructions =
        QObject::tr(
            "Install ffmpeg with your distribution's package manager:\n"
            "\n"
            "  • Debian / Ubuntu : sudo apt install ffmpeg\n"
            "  • Fedora / RHEL   : sudo dnf install ffmpeg\n"
            "  • Arch / Manjaro  : sudo pacman -S ffmpeg\n"
            "  • openSUSE        : sudo zypper install ffmpeg\n"
            "  • Flatpak         : flatpak install flathub org.ffmpeg.FFmpeg\n"
            "\n"
            "More info: %1")
        .arg(h.help_url);
#endif
    return h;
}

// Returns the path to ffmpeg: checks the application's own directory first
// (for portable/bundled installs), then falls back to a PATH search.
QString find_ffmpeg() {
    // 1. Next to the executable (portable bundle)
    QString beside = QDir(QCoreApplication::applicationDirPath()).filePath(
#ifdef Q_OS_WIN
        "ffmpeg.exe"
#else
        "ffmpeg"
#endif
    );
    if(QFileInfo::exists(beside)) {
        return beside;
    }
    // 2. Anywhere on PATH
    return QStandardPaths::findExecutable("ffmpeg");
}

bool ffmpeg_is_available() {
    return !find_ffmpeg().isEmpty();
}

// Block the export until ffmpeg is on PATH. Returns true if ffmpeg is now
// available; false if the user cancelled or refused to install.
bool require_ffmpeg(QWidget* parent) {
    while(!ffmpeg_is_available()) {
        FfmpegInstallHelp help = ffmpeg_install_help();

        QMessageBox box(parent);
        box.setIcon(QMessageBox::Warning);
        box.setWindowTitle(QObject::tr("ffmpeg required"));
        box.setText(QObject::tr(
            "Saving the dynamic simulation as a video requires <b>ffmpeg</b>, which is not "
            "installed (or not on PATH).<br><br>"
            "Please install ffmpeg, then click <b>Recheck</b> to continue."));
        box.setInformativeText(help.instructions);

        QPushButton* recheck_btn = box.addButton(QObject::tr("Recheck"), QMessageBox::AcceptRole);
        QPushButton* visit_btn   = box.addButton(QObject::tr("Open ffmpeg.org"), QMessageBox::HelpRole);
        QPushButton* cancel_btn  = box.addButton(QMessageBox::Cancel);
        box.setDefaultButton(recheck_btn);
        box.exec();

        QAbstractButton* clicked = box.clickedButton();
        if(clicked == cancel_btn || clicked == nullptr) {
            return false;
        }
        if(clicked == visit_btn) {
            QDesktopServices::openUrl(QUrl(help.help_url));
            // Fall through to loop iteration so the "Recheck" attempt happens
            // on the next pass after the user installs.
            continue;
        }
        // Recheck: loop iterates and re-tests ffmpeg_is_available().
    }
    return true;
}

} // namespace

namespace {

// Concatenate the geometric arrays of static and dynamic States into a single
// States object, leaving non-geometric fields default-constructed (ShapePlot
// only consults the geometric ones, plus `time.size()` for ghost sampling
// and updateAxes()).
States build_combined_states(const States& s, const States& d) {
    States c;

    // `time` deliberately holds ONLY the static entries. ShapePlot uses
    // `time.size()` to (a) sample the gray ghost background states evenly
    // across the timeline and (b) compute axis ranges. Restricting it to
    // the static portion keeps the ghosts where they belong (showing the
    // pulling progression) and prevents updateAxes() from using outdated
    // ranges — we override the axes manually below.
    c.time = s.time;

    auto append = [](auto& dst, const auto& a, const auto& b) {
        dst.reserve(a.size() + b.size());
        dst.insert(dst.end(), a.begin(), a.end());
        dst.insert(dst.end(), b.begin(), b.end());
    };

    append(c.limb_pos,         s.limb_pos,         d.limb_pos);
    append(c.lower_limb_pos,   s.lower_limb_pos,   d.lower_limb_pos);
    append(c.string_pos,       s.string_pos,       d.string_pos);
    append(c.arrow_pos,        s.arrow_pos,        d.arrow_pos);

    return c;
}

// Compute axis ranges over the union of static + dynamic geometric data,
// matching the convention used by ShapePlot::updateAxes().
void compute_unified_ranges(const Common& common,
                            const States& s,
                            const States& d,
                            QCPRange& x_range,
                            QCPRange& y_range)
{
    const auto& unit = Quantities::length.getUnit();

    auto expand2 = [&](const std::vector<std::array<double, 2>>& pos) {
        for(const auto& p : pos) {
            x_range.expand(unit.fromBase(p[0]));
            y_range.expand(unit.fromBase(p[1]));
        }
    };
    auto expand3 = [&](const std::vector<std::array<double, 3>>& pos) {
        for(const auto& p : pos) {
            x_range.expand(unit.fromBase(p[0]));
            y_range.expand(unit.fromBase(p[1]));
        }
    };

    expand3(common.limb.position_eval);
    expand3(common.limb_lower.position_eval);

    auto expand_states = [&](const States& st) {
        for(size_t i = 0; i < st.time.size(); ++i) {
            expand3(st.limb_pos[i]);
            expand3(st.lower_limb_pos[i]);
            expand2(st.string_pos[i]);
        }
    };
    expand_states(s);
    expand_states(d);
}

} // namespace

ShapeVideoExporter::ShapeVideoExporter(QWidget* parent_widget, ShapePlot* size_reference,
                                       const BowResult& data,
                                       double hold_seconds, double static_seconds,
                                       double dynamic_seconds)
    : QObject(parent_widget),
      parent_widget(parent_widget),
      size_reference(size_reference),
      data(data),
      hold_seconds(hold_seconds),
      static_seconds(static_seconds),
      dynamic_seconds(dynamic_seconds)
{
    // Pre-build the combined States now so the off-screen ShapePlot can hold
    // a stable const reference into it for the rest of the exporter's life.
    if(data.statics.has_value() && data.dynamics.has_value()) {
        combined_states = build_combined_states(data.statics->states, data.dynamics->states);
        n_static = static_cast<int>(data.statics->states.time.size());
        n_dynamic = static_cast<int>(data.dynamics->states.time.size());
    }
}

ShapeVideoExporter::~ShapeVideoExporter() = default;

bool ShapeVideoExporter::run() {
    if(!data.dynamics.has_value() || n_dynamic < 2) {
        QMessageBox::warning(parent_widget, tr("Save as video"),
                             tr("There are not enough simulation states to export a video."));
        return false;
    }

    // Hard requirement: refuse to proceed without ffmpeg.
    if(!require_ffmpeg(parent_widget)) {
        return false;
    }

    QString filter = tr("MP4 video (*.mp4);;WebM video (*.webm);;Animated GIF (*.gif)");
    QString suggested_name = QStringLiteral("simulation.mp4");
    QString out_path = QFileDialog::getSaveFileName(
        parent_widget, tr("Save dynamic simulation as video"),
        QDir(QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation)).filePath(suggested_name),
        filter);

    if(out_path.isEmpty()) {
        return false;
    }

    bool ok = false;
    int fps = QInputDialog::getInt(parent_widget, tr("Save as video"),
                                   tr("Frames per second:"), 30, 1, 120, 1, &ok);
    if(!ok) {
        return false;
    }

    QFileInfo fi(out_path);
    QString ext = fi.suffix().toLower();
    if(ext.isEmpty() || (ext != "mp4" && ext != "webm" && ext != "gif")) {
        ext = "mp4";
        out_path += "." + ext;
        fi = QFileInfo(out_path);
    }

    // Render every state into a temporary directory as PNG frames.
    QTemporaryDir tmp;
    tmp.setAutoRemove(true);
    if(!tmp.isValid()) {
        QMessageBox::critical(parent_widget, tr("Save as video"),
                              tr("Could not create a temporary directory for the video frames."));
        return false;
    }

    // Sample the size of the visible plot once so the output frame size
    // matches what the user sees and stays constant across the whole video.
    const QSize frame_size = size_reference->size();
    if(frame_size.width() < 16 || frame_size.height() < 16) {
        QMessageBox::warning(parent_widget, tr("Save as video"),
                             tr("The shape plot is too small to render. Resize the window and try again."));
        return false;
    }

    // Build the off-screen ShapePlot lazily on the first run(). It uses the
    // combined states (static + dynamic) so a single state index drives the
    // entire video, guaranteeing identical bow geometry at the boundary
    // between the pulling and release phases. background_states=4 reproduces
    // the gray ghost shapes from the static plot during BOTH phases.
    if(video_plot == nullptr) {
        constexpr int kBackgroundStates = 4;
        video_plot = new ShapePlot(data.common, combined_states, kBackgroundStates);
        video_plot->setParent(parent_widget);
        video_plot->setVisible(false);
    }

    // Force a constant render size and unified axis range so neither pans
    // nor zooms across the video.
    video_plot->resize(frame_size);
    {
        QCPRange xr, yr;
        compute_unified_ranges(data.common, data.statics->states, data.dynamics->states, xr, yr);
        video_plot->setAxesLimits(1.05 * xr, 1.05 * yr);    // matches ShapePlot::updateAxes()
    }

    // Compose the frame sequence: static (pulling) -> hold @ full draw -> dynamic (release).
    //
    // The static simulation typically only produces a small number of states
    // (driven by `n_draw_steps`, often 25-50). Emitting one output frame per
    // state at e.g. 30 fps would cram the pulling phase into well under a
    // second with each consecutive bow shape visibly different — the user
    // perceives this as choppy motion. We stretch the phase to
    // `static_seconds` of playback by repeating each source state across
    // multiple output frames (a step interpolation).
    const int n_prefix_frames = (n_static > 0)
        ? std::max(n_static, static_cast<int>(std::lround(static_seconds * fps)))
        : 0;
    const int n_hold = (n_static > 0)
        ? std::max(0, static_cast<int>(std::lround(hold_seconds * fps)))
        : 0;

    // Resample the dynamic phase to uniform time intervals so the arrow moves
    // at visually constant speed. The adaptive ODE solver produces states at
    // non-uniform timestamps; emitting one frame per state would make the
    // arrow appear to speed up and slow down erratically.
    //
    // The physical release is very short (typically 10-30 ms) — at 30 fps
    // that's well under one frame. We stretch it to `dynamic_seconds` of
    // playback (slow motion) so the user can actually see the arrow fly,
    // never dropping below one frame per source state.
    const auto& dyn_time = data.dynamics->states.time;
    const double dynamic_duration = dyn_time.back() - dyn_time.front();
    const int n_dynamic_realtime = std::max(2, static_cast<int>(std::lround(dynamic_duration * fps)));
    const int n_dynamic_slowmo   = std::max(2, static_cast<int>(std::lround(dynamic_seconds * fps)));
    const int n_dynamic_frames   = std::max({n_dynamic_realtime, n_dynamic_slowmo, n_dynamic});

    const int total_frames = n_prefix_frames + n_hold + n_dynamic_frames;

    // Indices into `combined_states`:
    //   static phase  : 0 .. n_static - 1
    //   dynamic phase : n_static .. n_static + n_dynamic - 1
    const int dynamic_offset = n_static;

    QProgressDialog progress(tr("Rendering frames..."), tr("Cancel"), 0, total_frames, parent_widget);
    progress.setWindowModality(Qt::WindowModal);
    progress.setMinimumDuration(0);

    int frame_idx = 0;
    QPixmap cached;
    int cached_state = -1;
    auto write_frame = [&](int combined_state_idx) -> bool {
        progress.setValue(frame_idx);
        QApplication::processEvents();
        if(progress.wasCanceled()) {
            return false;
        }

        if(combined_state_idx != cached_state) {
            video_plot->setStateIndex(combined_state_idx);
            video_plot->replot(QCustomPlot::rpImmediateRefresh);
            cached = video_plot->toPixmap(frame_size.width(), frame_size.height());
            cached_state = combined_state_idx;
        }
        QString frame_path = tmp.filePath(QString("frame_%1.png").arg(frame_idx, 6, 10, QChar('0')));
        if(!cached.save(frame_path, "PNG")) {
            QMessageBox::critical(parent_widget, tr("Save as video"),
                                  tr("Failed to write frame %1.").arg(frame_idx));
            return false;
        }
        ++frame_idx;
        return true;
    };

    // Phase 1: static (pulling) frames, stretched to `static_seconds`.
    //
    // Important: the static and dynamic simulations each capture their own
    // "bow at full draw" state. combined_states[n_static - 1] (final static)
    // and combined_states[dynamic_offset] (first dynamic) describe the same
    // physical configuration, but were produced by different solvers, so
    // their limb meshes / string polylines differ slightly. Rendering one
    // immediately after the other shows that small numerical mismatch as a
    // visible pop right at the static -> dynamic boundary.
    //
    // To eliminate the pop, we make phase 1 ramp end on `dynamic_offset`
    // (the first DYNAMIC capture of full draw), then hold there, then start
    // phase 3 from the same index. The boundary frames are then literally
    // the same data — no jump.
    const int ramp_end_idx = (n_static > 0) ? dynamic_offset : 0;
    for(int i = 0; i < n_prefix_frames; ++i) {
        int src_idx;
        if(n_prefix_frames <= 1 || n_static <= 1) {
            src_idx = ramp_end_idx;
        } else {
            // Even step distribution: 0 -> 0, n_prefix_frames-1 -> ramp_end_idx.
            src_idx = static_cast<int>(std::lround(
                static_cast<double>(i) * ramp_end_idx / (n_prefix_frames - 1)));
            // Skip combined index n_static - 1 (the redundant static-side
            // capture of full draw); it would otherwise produce a one-frame
            // pop just before reaching ramp_end_idx.
            if(src_idx == n_static - 1) {
                src_idx = ramp_end_idx;
            }
        }
        if(!write_frame(src_idx)) return false;
    }

    // Phase 2: hold at full draw, using the dynamic-side capture so that
    // phase 3 (which starts at the same index) continues seamlessly.
    for(int i = 0; i < n_hold; ++i) {
        if(!write_frame(ramp_end_idx)) return false;
    }

    // Phase 3: dynamic (release) frames, resampled at uniform time intervals.
    // For each output frame, find the nearest simulation state by timestamp.
    // The first frame maps to dynamic_offset (same as the hold frame above)
    // so there is no transition pop.
    for(int i = 0; i < n_dynamic_frames; ++i) {
        const double target_t = dyn_time.front()
            + static_cast<double>(i) * dynamic_duration / (n_dynamic_frames - 1);
        auto it = std::lower_bound(dyn_time.begin(), dyn_time.end(), target_t);
        int dyn_idx;
        if(it == dyn_time.end()) {
            dyn_idx = n_dynamic - 1;
        } else if(it == dyn_time.begin()) {
            dyn_idx = 0;
        } else {
            auto prev = std::prev(it);
            dyn_idx = (target_t - *prev <= *it - target_t)
                ? static_cast<int>(prev - dyn_time.begin())
                : static_cast<int>(it  - dyn_time.begin());
        }
        if(!write_frame(dynamic_offset + dyn_idx)) return false;
    }
    progress.setValue(total_frames);

    // Build ffmpeg command line. For MP4 we use H.264 with yuv420p so the
    // file plays in any standard player. Width/height are forced to even
    // numbers because H.264 requires it.
    QProgressDialog enc_progress(tr("Encoding video with ffmpeg..."), QString(), 0, 0, parent_widget);
    enc_progress.setWindowModality(Qt::WindowModal);
    enc_progress.setMinimumDuration(0);
    enc_progress.show();
    QApplication::processEvents();

    QStringList args;
    args << "-y"
         << "-framerate" << QString::number(fps)
         << "-i" << tmp.filePath("frame_%06d.png");

    if(ext == "mp4") {
        args << "-c:v" << "libx264"
             << "-pix_fmt" << "yuv420p"
             << "-vf" << "scale=trunc(iw/2)*2:trunc(ih/2)*2"
             << "-movflags" << "+faststart";
    } else if(ext == "webm") {
        args << "-c:v" << "libvpx-vp9"
             << "-pix_fmt" << "yuv420p"
             << "-vf" << "scale=trunc(iw/2)*2:trunc(ih/2)*2"
             << "-b:v" << "0" << "-crf" << "32";
    } else if(ext == "gif") {
        args << "-vf" << "fps=" + QString::number(fps) + ",split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse";
    }
    args << out_path;

    QProcess ffmpeg;
    ffmpeg.setProcessChannelMode(QProcess::MergedChannels);
    ffmpeg.start(find_ffmpeg(), args);
    if(!ffmpeg.waitForStarted(5000)) {
        QMessageBox::critical(parent_widget, tr("Save as video"),
                              tr("Could not launch ffmpeg."));
        return false;
    }
    if(!ffmpeg.waitForFinished(-1)) {
        QMessageBox::critical(parent_widget, tr("Save as video"),
                              tr("ffmpeg did not finish."));
        return false;
    }
    enc_progress.close();

    if(ffmpeg.exitStatus() != QProcess::NormalExit || ffmpeg.exitCode() != 0) {
        QString log = QString::fromLocal8Bit(ffmpeg.readAll());
        QMessageBox box(QMessageBox::Critical, tr("Save as video"),
                        tr("ffmpeg failed (exit code %1).").arg(ffmpeg.exitCode()),
                        QMessageBox::Ok, parent_widget);
        box.setDetailedText(log);
        box.exec();
        return false;
    }

    QMessageBox::information(parent_widget, tr("Save as video"),
                             tr("Saved %1 frames at %2 fps to:\n%3")
                                 .arg(total_frames).arg(fps).arg(out_path));
    return true;
}
