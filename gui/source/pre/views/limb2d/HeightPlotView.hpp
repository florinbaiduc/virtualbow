#pragma once
#include "pre/widgets/PlotWidget.hpp"
#include "solver/BowModel.hpp"

class MainModel;
class QCPGraph;

class HeightPlotView: public PlotWidget {
public:
    HeightPlotView(MainModel* model, LimbSide side, QPersistentModelIndex index);

private:
    MainModel* model;
    LimbSide side;
    QPersistentModelIndex index;

    QCPGraph* graphLine;
    QCPGraph* graphPoints;
    QCPGraph* graphSelected;

    void updatePlot();
    void setNodesVisible(bool visible);
};
