#pragma once
#include "pre/widgets/PlotWidget.hpp"
#include "solver/BowModel.hpp"

class MainModel;
class QCPGraph;

class WidthPlotView: public PlotWidget {
public:
    WidthPlotView(MainModel* model, LimbSide side);

private:
    MainModel* model;
    LimbSide side;
    QCPGraph* graphLine;
    QCPGraph* graphPoints;
    QCPGraph* graphSelected;

    void updatePlot();
    void setNodesVisible(bool visible);
};
