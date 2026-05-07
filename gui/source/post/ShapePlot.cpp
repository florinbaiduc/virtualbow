#include "ShapePlot.hpp"
#include "pre/models/units/UnitSystem.hpp"
#include <algorithm>
#include <cmath>

ShapePlot::ShapePlot(const Common& common, const States& states, int background_states)
    : common(common),
      states(states),
      quantity(Quantities::length),
      background_states(background_states),
      index(0)
{
    this->setAspectPolicy(PlotWidget::SCALE_Y);

    // Curves for background states (index: 0 ... background_states - 1)

    auto addBackgroundCurve = [&](QList<QCPCurve*>& list) {
        list.append(new QCPCurve(this->xAxis, this->yAxis));
        list.back()->setPen({Qt::lightGray, 1.0});
        list.back()->setScatterSkip(0);
    };

    for(int i = 0; i < background_states; ++i) {
        addBackgroundCurve(limb_upper);
        addBackgroundCurve(limb_lower);
        addBackgroundCurve(string_curves);
        addBackgroundCurve(handle_curves);
    }

    // Curves for current state (index: background_state)

    limb_upper.append(new QCPCurve(this->xAxis, this->yAxis));
    limb_upper.back()->setName("Upper limb");
    limb_upper.back()->setPen({Qt::blue, 2.0});
    limb_upper.back()->setScatterSkip(0);

    limb_lower.append(new QCPCurve(this->xAxis, this->yAxis));
    limb_lower.back()->setName("Lower limb");
    limb_lower.back()->setPen({Qt::blue, 2.0});
    limb_lower.back()->setScatterSkip(0);

    string_curves.append(new QCPCurve(this->xAxis, this->yAxis));
    string_curves.back()->setName("String");
    string_curves.back()->setPen({Qt::blue, 2.0});
    string_curves.back()->setScatterSkip(0);

    handle_curves.append(new QCPCurve(this->xAxis, this->yAxis));
    handle_curves.back()->setName("Handle");
    handle_curves.back()->setPen({Qt::black, 1.5});
    handle_curves.back()->setBrush(QBrush(QColor(40, 40, 40)));
    handle_curves.back()->setScatterSkip(0);

    pivot = new QCPCurve(this->xAxis, this->yAxis);
    pivot->setName("Pivot");
    pivot->setLineStyle(QCPCurve::lsNone);
    pivot->setScatterStyle({QCPScatterStyle::ssCross, Qt::blue, 10});

    arrow = new QCPCurve(this->xAxis, this->yAxis);
    arrow->setName("Arrow");
    arrow->setLineStyle(QCPCurve::lsNone);
    arrow->setScatterStyle({QCPScatterStyle::ssCrossCircle, Qt::red, 10});

    QObject::connect(&quantity, &Quantity::unitChanged, this, &ShapePlot::updatePlot);
    updatePlot();
}

void ShapePlot::setStateIndex(int i) {
    index = i;
    updateCurrentState();
    this->replot();
}

void ShapePlot::updatePlot() {
    updateBackgroundStates();
    updateCurrentState();
    updateAxes();

    this->replot();
}

void ShapePlot::updateBackgroundStates() {

    int intermediate_states = background_states - 1;    // Number of states from brace to full draw, excluding the unbraced state

    for(int i = 0; i < intermediate_states; ++i) {
        size_t j = (intermediate_states == 1) ? 0 : i*(states.time.size() - 1)/(intermediate_states - 1);
        plotLimbOutline(limb_upper[i], common.limb,       states.limb_pos[j], LimbSide::Upper);
        plotLimbOutline(limb_lower[i], common.limb_lower, states.lower_limb_pos[j], LimbSide::Lower);
        plotString(string_curves[i], states.string_pos[j]);
        plotHandle(handle_curves[i], states.limb_pos[j].front(), states.lower_limb_pos[j].front());
    }

    if(intermediate_states >= 0) {
        // Unbraced state
        plotLimbOutline(limb_upper[intermediate_states], common.limb,       common.limb.position_eval,       LimbSide::Upper);
        plotLimbOutline(limb_lower[intermediate_states], common.limb_lower, common.limb_lower.position_eval, LimbSide::Lower);
        plotHandle(handle_curves[intermediate_states], common.limb.position_eval.front(), common.limb_lower.position_eval.front());
    }
}

void ShapePlot::updateCurrentState() {
    plotLimbOutline(limb_upper.back(), common.limb,       states.limb_pos[index],       LimbSide::Upper);
    plotLimbOutline(limb_lower.back(), common.limb_lower, states.lower_limb_pos[index], LimbSide::Lower);
    plotString(string_curves.back(), states.string_pos[index]);
    plotHandle(handle_curves.back(), states.limb_pos[index].front(), states.lower_limb_pos[index].front());
    plotArrow(states.arrow_pos[index]);
    plotPivot();
}

void ShapePlot::updateAxes() {
    this->xAxis->setLabel("X " + quantity.getUnit().getLabel());
    this->yAxis->setLabel("Y " + quantity.getUnit().getLabel());

    QCPRange x_range;
    QCPRange y_range;

    auto expand2 = [&](const std::vector<std::array<double, 2>>& position) {
        for(size_t i = 0; i < position.size(); ++i) {
            x_range.expand(quantity.getUnit().fromBase(position[i][0]));
            y_range.expand(quantity.getUnit().fromBase(position[i][1]));
        }
    };

    auto expand3 = [&](const std::vector<std::array<double, 3>>& position) {
        for(size_t i = 0; i < position.size(); ++i) {
            x_range.expand(quantity.getUnit().fromBase(position[i][0]));
            y_range.expand(quantity.getUnit().fromBase(position[i][1]));
        }
    };

    expand3(common.limb.position_eval);
    expand3(common.limb_lower.position_eval);
    for(size_t i = 0; i < states.time.size(); ++i) {
        expand3(states.limb_pos[i]);
        expand3(states.lower_limb_pos[i]);
        expand2(states.string_pos[i]);
        // Note: arrow_pos is NOT included here. While the arrow is on the
        // string its position equals string_pos[i][0] (the nock), so it is
        // already covered. Including the free-flight trajectory would expand
        // the y-range to follow the arrow as it flies away, which pushes the
        // bow to the bottom of the view and makes it invisible at state 0.
    }

    this->setAxesLimits(1.05*x_range, 1.05*y_range);    // Add a little extra space in the x and y directions
}

void ShapePlot::plotLimbOutline(QCPCurve* curve, const LimbInfo& limb, const std::vector<std::array<double, 3>>& position, LimbSide side) {
    curve->data()->clear();

    // The lower limb's positions are stored in the world frame after the
    // x-mirror (x, y, φ) → (-x, y, π−φ). The mirror preserves the y-component
    // of the cross-section normal, but reconstructing it from the stored
    // angle as (-sin φ, cos φ) negates that y-component. To draw the back
    // and belly edges on the correct side of the lower limb (so symmetric
    // bows like the flatbow join continuously at the grip), we negate the
    // standard normal for the lower limb.
    const double s = (side == LimbSide::Upper) ? 1.0 : -1.0;

    // Forward sweep: back-side edge of the cross-section
    for(int i = 0; i < (int)position.size(); ++i) {
        double xi = position[i][0] - s*limb.bounds[i].back()*sin(position[i][2]);
        double yi = position[i][1] + s*limb.bounds[i].back()*cos(position[i][2]);
        curve->addData(
            quantity.getUnit().fromBase(xi),
            quantity.getUnit().fromBase(yi)
        );
    }

    // Backward sweep: belly-side edge, closing the outline
    for(int i = (int)position.size() - 1; i >= 0; --i) {
        double xi = position[i][0] - s*limb.bounds[i].front()*sin(position[i][2]);
        double yi = position[i][1] + s*limb.bounds[i].front()*cos(position[i][2]);
        curve->addData(
            quantity.getUnit().fromBase(xi),
            quantity.getUnit().fromBase(yi)
        );
    }
}

void ShapePlot::plotString(QCPCurve* curve, const std::vector<std::array<double, 2>>& position) {
    curve->data()->clear();
    if(position.empty()) return;

    // The simulation produces `string_pos` in this storage order:
    //   index 0       : nock
    //   index 1..k    : upper-side contacts going OUTWARD from nock to upper_tip
    //   index k+1..N-1: lower-side contacts going OUTWARD from a point near
    //                   the nock down to lower_tip
    //
    // Drawing in storage order zig-zags from upper_tip back to near the nock.
    // The split index `k` is not stored, but the single largest jump between
    // consecutive points in storage order is always between `upper_tip` and
    // the first lower contact (they sit on opposite ends of the bow). Find
    // that gap, then emit one continuous polyline:
    //     lower_tip -> ... -> nock -> ... -> upper_tip.
    const size_t N = position.size();
    if(N == 1) {
        curve->addData(
            quantity.getUnit().fromBase(position[0][0]),
            quantity.getUnit().fromBase(position[0][1])
        );
        return;
    }

    size_t split = N;  // index of the first lower-side point; N means no lower
    double max_gap = -1.0;
    for(size_t i = 1; i < N; ++i) {
        double dx = position[i][0] - position[i-1][0];
        double dy = position[i][1] - position[i-1][1];
        double d2 = dx*dx + dy*dy;
        if(d2 > max_gap) {
            max_gap = d2;
            split = i;
        }
    }

    auto add = [&](const std::array<double, 2>& p) {
        curve->addData(
            quantity.getUnit().fromBase(p[0]),
            quantity.getUnit().fromBase(p[1])
        );
    };

    // Lower half: positions [split..N-1] are stored nock-side -> lower_tip.
    // Walk it in reverse so the polyline starts at lower_tip.
    for(size_t i = N; i > split; --i) {
        add(position[i - 1]);
    }
    // Upper half (including the nock at index 0): nock -> upper_tip.
    for(size_t i = 0; i < split; ++i) {
        add(position[i]);
    }
}

void ShapePlot::plotHandle(QCPCurve* curve, const std::array<double, 3>& upper_inboard, const std::array<double, 3>& lower_inboard) {
    // Draw the grip as a closed rectangle whose long axis is the line between
    // the two inboard limb tips and whose depth matches the limb laminate
    // thickness, so the rectangle's long edges line up with the limb back
    // and belly outlines.
    //
    // Building this polygon from each limb's local back/belly normal (as the
    // outline drawing does) self-intersects into a bow-tie whenever the
    // upper and lower limbs meet at near-opposite tangents -- e.g. a yumi
    // with a short rigid handle, or any continuous-joint topology -- because
    // the lower-side normal flip puts the lower back/belly corners on the
    // same side as the upper ones. Using the grip-axis perpendicular avoids
    // that failure mode for every bow geometry.
    curve->data()->clear();

    // Limb half-thickness at the inboard joint, taken as the larger of the
    // two laminate extents on each side.
    const double tu = std::max(std::abs(common.limb.bounds.front().back()),
                               std::abs(common.limb.bounds.front().front()));
    const double tl = std::max(std::abs(common.limb_lower.bounds.front().back()),
                               std::abs(common.limb_lower.bounds.front().front()));
    const double half_depth = std::max(tu, tl);
    if(half_depth <= 0.0) {
        return;
    }

    const double dx = upper_inboard[0] - lower_inboard[0];
    const double dy = upper_inboard[1] - lower_inboard[1];
    const double grip_length = std::sqrt(dx*dx + dy*dy);

    // If the two inboard tips are closer together than the limb thickness,
    // the limbs already meet directly (continuous-joint / yumi-style bow):
    // there's no meaningful handle block to draw, and any non-zero polygon
    // we'd construct here would just produce the bow-tie artefact again.
    if(grip_length < 2.0 * half_depth) {
        return;
    }

    // Unit perpendicular to the grip axis.
    const double px = -dy / grip_length;
    const double py =  dx / grip_length;

    auto add = [&](double x, double y) {
        curve->addData(quantity.getUnit().fromBase(x), quantity.getUnit().fromBase(y));
    };

    // Closed CCW rectangle.
    add(lower_inboard[0] + half_depth*px, lower_inboard[1] + half_depth*py);
    add(upper_inboard[0] + half_depth*px, upper_inboard[1] + half_depth*py);
    add(upper_inboard[0] - half_depth*px, upper_inboard[1] - half_depth*py);
    add(lower_inboard[0] - half_depth*px, lower_inboard[1] - half_depth*py);
    add(lower_inboard[0] + half_depth*px, lower_inboard[1] + half_depth*py);
}

void ShapePlot::plotArrow(double position) {
    // The arrow rides on the nock, which is offset from x=0 by `nock_offset`
    // for asymmetric bows (e.g. a yumi). Plotting at x=0 made the arrow
    // marker visibly drift away from where the string actually bends.
    arrow->data()->clear();
    arrow->addData(
        quantity.getUnit().fromBase(common.nock_offset),
        quantity.getUnit().fromBase(position)
    );
}

void ShapePlot::plotPivot() {
    // Pivot is on the bow's longitudinal axis at x=0; only its y-coordinate
    // is offset from the bow center.
    pivot->data()->clear();
    pivot->addData(
        quantity.getUnit().fromBase(0.0),
        quantity.getUnit().fromBase(common.pivot_point)
    );
}
