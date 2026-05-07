#pragma once
#include "OpenGLUtils.hpp"
#include "solver/BowModel.hpp"
#include <qopengl.h>

class BowModel;
class LimbInfo;
class QVector3D;
class QColor;

class LimbMesh
{
public:
    Mesh faces_right;
    Mesh faces_left;

    LimbMesh(const BowModel& bow, const LimbInfo& geometry, LimbSide side);

private:
    bool reverse_winding = false;
    void addQuad(const QVector3D& p0, const QVector3D& p1, const QVector3D& p2, const QVector3D& p3, const QColor& color);
};
