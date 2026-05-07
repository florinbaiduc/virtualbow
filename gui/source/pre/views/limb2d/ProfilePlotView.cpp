#include "ProfilePlotView.hpp"
#include "pre/models/MainModel.hpp"
#include "pre/models/units/UnitSystem.hpp"

#include <QMouseEvent>
#include <cmath>
#include <variant>

// Magic number, determines offset of the curvature outline (fraction of curve length)
const double CURVATURE_SCALING  = 0.05;

// Hit-test radius in widget pixels around a draggable handle's centre.
static constexpr int HANDLE_HIT_RADIUS_PX = 10;

ProfilePlotView::ProfilePlotView(MainModel* model, LimbSide side):
    model(model),
    side(side)
{
    this->setAspectPolicy(PlotWidget::SCALE_Y);

    // Line
    curveLine = new QCPCurve(this->xAxis, this->yAxis);
    curveLine->setName("Line");
    curveLine->setPen({Qt::blue, 2});

    // Control points (segment endpoints reported by the solver — visual only)
    curveNodes = new QCPCurve(this->xAxis, this->yAxis);
    curveNodes->setName("Points");
    curveNodes->setScatterStyle({QCPScatterStyle::ssSquare, Qt::blue, 8});
    curveNodes->setLineStyle(QCPCurve::lsNone);

    // Draggable spline control-point handles (red, slightly larger so they
    // visually sit on top of the blue endpoint markers when they coincide).
    curveSplineHandles = new QCPCurve(this->xAxis, this->yAxis);
    curveSplineHandles->setName("Spline handles");
    QCPScatterStyle handleStyle(QCPScatterStyle::ssSquare, QPen(Qt::red, 1), QBrush(Qt::red), 10);
    curveSplineHandles->setScatterStyle(handleStyle);
    curveSplineHandles->setLineStyle(QCPCurve::lsNone);

    // Curvature visualization
    curvatureOutline = new QCPCurve(this->xAxis, this->yAxis);
    curvatureOutline->setName("Curvature outline");
    curvatureOutline->setPen(Qt::NoPen);
    curvatureOutline->setBrush(QBrush(QColor(0, 0, 255, 35)));
    curvatureOutline->setScatterSkip(0);

    // Track the mouse to give visual feedback (cursor change) when hovering
    // a draggable handle, even without a button held down.
    this->setMouseTracking(true);

    // Menu actions

    action_show_curvature = new QAction("Show curvature", this);
    action_show_curvature->setCheckable(true);
    action_show_curvature->setChecked(false);
    QObject::connect(action_show_curvature, &QAction::triggered, [&] {
        updateVisibility();
        replot();
    });

    action_show_nodes = new QAction("Show nodes", this);
    action_show_nodes->setCheckable(true);
    action_show_nodes->setChecked(true);
    QObject::connect(action_show_nodes, &QAction::triggered, [&] {
        updateVisibility();
        replot();
    });

    QAction* before = contextMenu()->actions().isEmpty() ? nullptr : contextMenu()->actions().front();
    contextMenu()->insertAction(before, action_show_curvature);
    contextMenu()->insertAction(before, action_show_nodes);
    contextMenu()->insertSeparator(before);

    // Update on unit and geometry changes
    QObject::connect(&Quantities::length, &Quantity::unitChanged, this, &ProfilePlotView::updatePlot);
    QObject::connect(model, &MainModel::geometryChanged, this, &ProfilePlotView::updatePlot);

    // Initial update
    updatePlot();
}

void ProfilePlotView::updatePlot() {
    this->xAxis->setLabel("X " + Quantities::length.getUnit().getLabel());
    this->yAxis->setLabel("Y " + Quantities::length.getUnit().getLabel());

    curveLine->data()->clear();
    curveNodes->data()->clear();
    curveSplineHandles->data()->clear();
    curvatureOutline->data()->clear();


    if(model->hasGeometry()) {
        const LimbInfo& info = (side == LimbSide::Upper) ? model->getGeometry().upper : model->getGeometry().lower;
        auto& position = info.position_eval;
        auto& curvature = info.curvature_eval;

        // Determine scaling of the curvature outline from curve length and maximum curvature
        double k_max = *std::max_element(curvature.begin(), curvature.end(), [](double a, double b){ return std::abs(a) < std::abs(b); });
        double scale = (k_max != 0.0) ? CURVATURE_SCALING*info.length.back()/std::abs(k_max) : 0.0;

        // Plot outline of the curvature curve
        for(size_t i = 0; i < position.size(); ++i) {
            curvatureOutline->addData(
                Quantities::length.getUnit().fromBase(position[i][0] - scale*curvature[i]*sin(position[i][2])),
                Quantities::length.getUnit().fromBase(position[i][1] + scale*curvature[i]*cos(position[i][2]))
            );
        }

        // Close the loop by adding the profile curve points
        for(int i = static_cast<int>(position.size()) - 1; i >= 0; --i) {
            curvatureOutline->addData(
                Quantities::length.getUnit().fromBase(position[i][0]),
                Quantities::length.getUnit().fromBase(position[i][1])
            );
        }

        // Plot profile curve
        for(auto& point: info.position_eval) {
            curveLine->addData(
                Quantities::length.getUnit().fromBase(point[0]),
                Quantities::length.getUnit().fromBase(point[1])
            );
        }

        // Plot profile nodes (segment endpoints from solver)
        for(auto& point: info.position_control) {
            curveNodes->addData(
                Quantities::length.getUnit().fromBase(point[0]),
                Quantities::length.getUnit().fromBase(point[1])
            );
        }
    }

    rebuildHandles();
    updateVisibility();
    rescaleAxes();
    replot();
}

void ProfilePlotView::rebuildHandles() {
    handles.clear();

    if(!model->hasBow() || !model->hasGeometry()) {
        return;
    }

    const BowModel& bow = model->getBow();
    const LimbInfo& info = (side == LimbSide::Upper) ? model->getGeometry().upper : model->getGeometry().lower;
    const auto& segments = (side == LimbSide::Upper) ? bow.profile.upper : bow.profile.lower;

    // For the lower limb the world frame is mirrored about the y-axis relative
    // to the segment-local frame in which Spline::points are stored, so its
    // local x maps to the negative of the world x-offset.
    const double xSign = (side == LimbSide::Upper) ? +1.0 : -1.0;

    // The solver returns position_control[k] = world-frame start of segment k
    // (and position_control.back() = end of last segment). If for any reason
    // we have fewer entries than segments, just bail out — the plot still
    // shows the read-only blue endpoint markers.
    if(info.position_control.size() < segments.size()) {
        return;
    }

    int segIndex = 0;
    for(const ProfileSegment& segment : segments) {
        if(std::holds_alternative<Spline>(segment)) {
            const Spline& spline = std::get<Spline>(segment);
            const double segStartX = info.position_control[segIndex][0];
            const double segStartY = info.position_control[segIndex][1];

            for(int j = 0; j < (int)spline.points.size(); ++j) {
                const double localX = spline.points[j][0];
                const double localY = spline.points[j][1];
                const double worldX = segStartX + xSign * localX;
                const double worldY = segStartY + localY;

                handles.push_back({segIndex, j, segStartX, segStartY, worldX, worldY});

                curveSplineHandles->addData(
                    Quantities::length.getUnit().fromBase(worldX),
                    Quantities::length.getUnit().fromBase(worldY)
                );
            }
        }
        ++segIndex;
    }
}

int ProfilePlotView::handleAtPixel(const QPoint& pos) const {
    int best = -1;
    double bestDist2 = double(HANDLE_HIT_RADIUS_PX) * double(HANDLE_HIT_RADIUS_PX);

    for(int i = 0; i < (int)handles.size(); ++i) {
        const double dispX = Quantities::length.getUnit().fromBase(handles[i].worldXBase);
        const double dispY = Quantities::length.getUnit().fromBase(handles[i].worldYBase);
        const double px = this->xAxis->coordToPixel(dispX);
        const double py = this->yAxis->coordToPixel(dispY);
        const double dx = px - pos.x();
        const double dy = py - pos.y();
        const double d2 = dx*dx + dy*dy;
        if(d2 <= bestDist2) {
            bestDist2 = d2;
            best = i;
        }
    }
    return best;
}

void ProfilePlotView::mousePressEvent(QMouseEvent* event) {
    if(event->button() == Qt::LeftButton) {
        const int hit = handleAtPixel(event->pos());
        if(hit >= 0) {
            activeHandle = hit;
            // Suppress QCustomPlot's range-drag while we drag the handle.
            this->axisRect()->setRangeDrag(Qt::Orientations());
            this->setCursor(Qt::ClosedHandCursor);
            event->accept();
            return;
        }
    }
    PlotWidget::mousePressEvent(event);
}

void ProfilePlotView::mouseMoveEvent(QMouseEvent* event) {
    if(activeHandle >= 0 && activeHandle < (int)handles.size()) {
        // Convert mouse pixels back to world coordinates in base SI units.
        const double dispX = this->xAxis->pixelToCoord(event->pos().x());
        const double dispY = this->yAxis->pixelToCoord(event->pos().y());
        const double worldX = Quantities::length.getUnit().toBase(dispX);
        const double worldY = Quantities::length.getUnit().toBase(dispY);

        const SplineHandle& h = handles[activeHandle];
        const double xSign = (side == LimbSide::Upper) ? +1.0 : -1.0;
        // Inverse of: world = segStart + sign*local  =>  local = sign*(world - segStart)
        const double localX = xSign * (worldX - h.segStartXBase);
        const double localY = worldY - h.segStartYBase;

        model->setSplinePoint(side, h.segIndex, h.pointIndex, localX, localY);
        event->accept();
        return;
    }

    // Provide hover feedback when not dragging.
    if(event->buttons() == Qt::NoButton) {
        if(handleAtPixel(event->pos()) >= 0) {
            this->setCursor(Qt::OpenHandCursor);
        } else {
            this->unsetCursor();
        }
    }

    PlotWidget::mouseMoveEvent(event);
}

void ProfilePlotView::mouseReleaseEvent(QMouseEvent* event) {
    if(activeHandle >= 0) {
        activeHandle = -1;
        // Restore default range-drag behaviour.
        this->axisRect()->setRangeDrag(Qt::Horizontal | Qt::Vertical);
        this->unsetCursor();
        event->accept();
        return;
    }
    PlotWidget::mouseReleaseEvent(event);
}

void ProfilePlotView::updateSelection() {
    /*
    for(int i = 0; i < segment_curves.size(); ++i) {
        if(selection.contains(i)) {
            segment_curves[i]->setPen({Qt::red, 2});
            segment_curves[i]->setScatterSkip(0);
        }
        else {
            segment_curves[i]->setPen({Qt::blue, 2});
            segment_curves[i]->setScatterSkip(0);
        }
    }

    for(int i = 0; i < segment_nodes.size(); ++i) {
        if(selection.contains(i) || selection.contains(i-1)) {
            segment_nodes[i]->setScatterStyle({QCPScatterStyle::ssSquare, Qt::red, 8});
            segment_nodes[i]->setLineStyle(QCPCurve::lsNone);
            segment_nodes[i]->setScatterSkip(0);
        }
        else {
            segment_nodes[i]->setScatterStyle({QCPScatterStyle::ssSquare, Qt::blue, 8});
            segment_nodes[i]->setLineStyle(QCPCurve::lsNone);
            segment_nodes[i]->setScatterSkip(0);
        }
    }
    */
}

void ProfilePlotView::updateVisibility() {
    const bool nodesVisible = action_show_nodes->isChecked();
    curveNodes->setVisible(nodesVisible);
    // Spline handles share visibility with the regular nodes — when nodes are
    // hidden, the user has explicitly opted out of point markers altogether.
    curveSplineHandles->setVisible(nodesVisible);
    curvatureOutline->setVisible(action_show_curvature->isChecked());
}
