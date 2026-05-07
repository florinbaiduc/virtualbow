#pragma once
#include "pre/widgets/PlotWidget.hpp"
#include "pre/models/units/Quantity.hpp"
#include "solver/BowModel.hpp"
#include "solver/BowResult.hpp"

class ShapePlot: public PlotWidget {
public:
    ShapePlot(const Common& common, const States& states, int background_states);
    void setStateIndex(int i);

    // Timer overlay (top-left of the plot). Shows elapsed time formatted as
    // "<ms>:<µs> ms:µs" with the microsecond field zero-padded to 3 digits.
    // The caller controls the time origin: pass 0 at the moment of release.
    void setTimerVisible(bool visible);
    void setTimerSeconds(double seconds);

private:
    const Common& common;
    const States& states;
    const Quantity& quantity;

    int background_states;
    int index;

    QList<QCPCurve*> limb_upper;
    QList<QCPCurve*> limb_lower;
    QList<QCPCurve*> string_curves;
    QList<QCPCurve*> handle_curves;     // Black line bridging the inboard limb tips, one per state
    QCPCurve* pivot;    // Todo: Replace with other QCustomPlot object?
    QCPCurve* arrow;    // Todo: Replace with other QCustomPlot object?
    QCPItemText* timer_label = nullptr;     // Elapsed-time overlay; hidden by default.

    void updatePlot();

    void updateBackgroundStates();
    void updateCurrentState();
    void updateAxes();

    void plotLimbOutline(QCPCurve* curve, const LimbInfo& limb, const std::vector<std::array<double, 3>>& position, LimbSide side);
    void plotString(QCPCurve* curve, const std::vector<std::array<double, 2>>& position);
    void plotHandle(QCPCurve* curve, const std::array<double, 3>& upper_inboard, const std::array<double, 3>& lower_inboard);
    void plotArrow(double position);
    void plotPivot();
};
