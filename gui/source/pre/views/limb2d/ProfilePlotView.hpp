#pragma once
#include "pre/widgets/PlotWidget.hpp"
#include "solver/BowModel.hpp"

#include <vector>

class MainModel;
class QMouseEvent;

class ProfilePlotView: public PlotWidget
{
public:
    ProfilePlotView(MainModel* model, LimbSide side);
    /*
    void setData(const Profile& data);
    void setSelection(const QList<int>& indices);
    */

protected:
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;

private:
    MainModel* model;
    LimbSide side;

    QAction* action_show_curvature;
    QAction* action_show_nodes;

    QCPCurve* curveLine;
    QCPCurve* curveNodes;
    QCPCurve* curveSplineHandles;
    QCPCurve* curvatureOutline;

    // One entry per draggable Spline control point currently shown.
    // Coordinates are stored in SI base units (metres) so dragging is independent
    // of the currently selected display unit.
    struct SplineHandle {
        int segIndex;        // index into profile segment list (upper or lower)
        int pointIndex;      // index into Spline::points
        double segStartXBase; // segment start in world frame (base SI)
        double segStartYBase;
        double worldXBase;   // current world position of this handle (base SI)
        double worldYBase;
    };
    std::vector<SplineHandle> handles;
    int activeHandle = -1; // index into `handles`, or -1 when not dragging

    void updatePlot();
    void rebuildHandles();
    void updateSelection();
    void updateVisibility();

    // Returns index into `handles` of the handle under the given widget-pixel
    // position, or -1 if none is within hit-test radius.
    int handleAtPixel(const QPoint& pos) const;
};
