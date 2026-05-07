#include "LimbView.hpp"
#include "LimbMesh.hpp"
#include "MaterialLegend.hpp"
#include "ErrorLabel.hpp"
#include "OpenGLUtils.hpp"
#include "pre/models/MainModel.hpp"
#include "pre/Language.hpp"
#include "solver/BowModel.hpp"
#include "config.hpp"
#include <QOpenGLShaderProgram>
#include <QMouseEvent>
#include <QCoreApplication>
#include <QToolButton>
#include <QPushButton>
#include <QHBoxLayout>
#include <QFrame>
#include <QLabel>
#include <QDate>
#include <QMenu>
#include <QFileDialog>
#include <QMessageBox>
#include <QPixmap>
#include <QPainter>

LimbView::LimbView(MainModel* model):
    model(model),
    materialLegend(new MaterialLegend()),
    background_shader(nullptr),
    model_shader(nullptr)
{
    // Anti aliasing
    QSurfaceFormat format = QSurfaceFormat::defaultFormat();
    format.setSamples(8);
    setFormat(format);

    const QSize BUTTON_SIZE = {34, 34};
    const QString BUTTON_STYLE = "QToolButton { background-color: rgba(220, 220, 220, 200); border-radius: 5px; }"
                                 "QToolButton:pressed { background-color: rgba(150, 150, 150, 200); }"
                                 "QToolButton:checked { background-color: rgba(150, 150, 150, 200); }";

    const QString BUTTON_FRAME_STYLE = "QFrame { background-color: rgba(38, 38, 38, 100); border-radius: 8px }";

    auto bt_view_3d = new QToolButton();
    QObject::connect(bt_view_3d, &QPushButton::clicked, this, &LimbView::view3D);
    bt_view_3d->setIcon(QIcon(":/icons/view-3d"));
    bt_view_3d->setToolTip(Tooltips::ViewDefault);
    bt_view_3d->setIconSize(BUTTON_SIZE);
    bt_view_3d->setStyleSheet(BUTTON_STYLE);

    auto bt_view_profile = new QToolButton();
    QObject::connect(bt_view_profile, &QPushButton::clicked, this, &LimbView::viewProfile);
    bt_view_profile->setIcon(QIcon(":/icons/view-profile"));
    bt_view_profile->setToolTip(Tooltips::ViewProfile);
    bt_view_profile->setIconSize(BUTTON_SIZE);
    bt_view_profile->setStyleSheet(BUTTON_STYLE);

    auto bt_view_top = new QToolButton();
    QObject::connect(bt_view_top, &QPushButton::clicked, this, &LimbView::viewTop);
    bt_view_top->setIcon(QIcon(":/icons/view-top"));
    bt_view_top->setToolTip(Tooltips::ViewBack);
    bt_view_top->setIconSize(BUTTON_SIZE);
    bt_view_top->setStyleSheet(BUTTON_STYLE);

    auto bt_view_fit = new QToolButton();
    QObject::connect(bt_view_fit, &QPushButton::clicked, this, &LimbView::viewFit);
    bt_view_fit->setIcon(QIcon(":/icons/view-fit"));
    bt_view_fit->setToolTip(Tooltips::ViewReset);
    bt_view_fit->setIconSize(BUTTON_SIZE);
    bt_view_fit->setStyleSheet(BUTTON_STYLE);

    auto buttonBox = new QHBoxLayout();
    buttonBox->setSpacing(12);
    buttonBox->setContentsMargins(12, 8, 12, 8);
    buttonBox->addWidget(bt_view_3d);
    buttonBox->addWidget(bt_view_profile);
    buttonBox->addWidget(bt_view_top);
    buttonBox->addWidget(bt_view_fit);

    auto buttonFrame = new QFrame();
    buttonFrame->setLayout(buttonBox);
    buttonFrame->setStyleSheet(BUTTON_FRAME_STYLE);

    errorLabel = new ErrorLabel();

    auto infoLabel = new QLabel("<font color=\"white\" size=\"7\">Virtual<strong>Bow</strong></font><br>"
                                "<font color=\"white\" size=\"3\">Noncommercial | v" + QString(Config::APPLICATION_VERSION) + "</font>");
    infoLabel->setAlignment(Qt::AlignRight);

    auto hbox = new QHBoxLayout();
    hbox->addWidget(materialLegend, 1, Qt::AlignTop);
    hbox->addWidget(errorLabel, 0, Qt::AlignCenter);
    hbox->addWidget(infoLabel, 1, Qt::AlignTop);

    auto vbox = new QVBoxLayout();
    hbox->setContentsMargins(10, 5, 10, 5);
    vbox->addLayout(hbox, 1);
    vbox->addWidget(buttonFrame, 0, Qt::AlignCenter);
    setLayout(vbox);

    // Context menu
    auto action_export = new QAction("Export as Image...", this);
    QObject::connect(action_export, &QAction::triggered, this, &LimbView::exportImage);

    auto menu = new QMenu(this);
    menu->addAction(action_export);

    this->setContextMenuPolicy(Qt::CustomContextMenu);
    QObject::connect(this, &QOpenGLWidget::customContextMenuRequested, [=, this](QPoint pos) {
        menu->exec(this->mapToGlobal(pos));
    });

    // v5: symmetry toggle removed; both upper and lower limbs are always rendered.
    view3D();

    // Update on changes to the geometry and once initially
    QObject::connect(model, &MainModel::geometryChanged, this, &LimbView::updateView);
    updateView();
}

void LimbView::updateView() {
    if(model->hasError()) {
        errorLabel->setText(model->getError());
        errorLabel->setVisible(true);
    }
    else {
        errorLabel->setText("");
        errorLabel->setVisible(false);
    }

    if(model->hasBow()) {
        materialLegend->setData(model->getBow().section.materials);

        if(model->hasGeometry()) {
            // v5: each LimbMesh already contains both lateral halves of its limb
            // (built directly from the centerline + width). The legacy x-mirror
            // copy in faces_left is no longer used; only the real meshes are
            // rendered, one per limb.
            LimbMesh meshUpper(model->getBow(), model->getGeometry().upper, LimbSide::Upper);
            LimbMesh meshLower(model->getBow(), model->getGeometry().lower, LimbSide::Lower);
            upper_right = std::make_unique<Model>(meshUpper.faces_right);
            lower_right = std::make_unique<Model>(meshLower.faces_right);
            upper_left  = nullptr;
            lower_left  = nullptr;

            // Build a solid black grip block between the two inboard limb tips
            // when a rigid handle is present. The block uses the inboard
            // cross-section of each limb so it joins both flush.
            handle = nullptr;
            if(auto* rigid = std::get_if<RigidHandle>(&model->getBow().handle)) {
                if(rigid->length_upper + rigid->length_lower > 0.0) {
                    const auto& upper = model->getGeometry().upper;
                    const auto& lower = model->getGeometry().lower;
                    const QColor BLACK(20, 20, 20);

                    // Upper inboard cross-section (at upper.position_eval[0])
                    auto upper_pos = upper.position_eval.front();
                    QVector3D upper_center((float)upper_pos[0], (float)upper_pos[1], 0.0f);
                    QVector3D upper_nw(0.0f, 0.0f, 1.0f);
                    QVector3D upper_nh((float)-sin(upper_pos[2]), (float)cos(upper_pos[2]), 0.0f);
                    float w_u = 0.5f*(float)upper.width.front();
                    float y_back_u  = (float)upper.bounds.front().back();   // y_back is the largest bound (back side)
                    float y_belly_u = (float)upper.bounds.front().front();

                    // Lower inboard cross-section (already in world frame; the
                    // sign flip of normal_h applied in LimbMesh applies here too).
                    auto lower_pos = lower.position_eval.front();
                    QVector3D lower_center((float)lower_pos[0], (float)lower_pos[1], 0.0f);
                    QVector3D lower_nw(0.0f, 0.0f, 1.0f);
                    QVector3D lower_nh(-(float)-sin(lower_pos[2]), -(float)cos(lower_pos[2]), 0.0f);
                    float w_l = 0.5f*(float)lower.width.front();
                    float y_back_l  = (float)lower.bounds.front().back();
                    float y_belly_l = (float)lower.bounds.front().front();

                    // Eight corners: U=upper, L=lower; B=back, b=belly; R=right, l=left
                    QVector3D UBR = upper_center + w_u*upper_nw + y_back_u*upper_nh;
                    QVector3D UBL = upper_center - w_u*upper_nw + y_back_u*upper_nh;
                    QVector3D UbR = upper_center + w_u*upper_nw + y_belly_u*upper_nh;
                    QVector3D UbL = upper_center - w_u*upper_nw + y_belly_u*upper_nh;

                    QVector3D LBR = lower_center + w_l*lower_nw + y_back_l*lower_nh;
                    QVector3D LBL = lower_center - w_l*lower_nw + y_back_l*lower_nh;
                    QVector3D LbR = lower_center + w_l*lower_nw + y_belly_l*lower_nh;
                    QVector3D LbL = lower_center - w_l*lower_nw + y_belly_l*lower_nh;

                    Mesh m(GL_QUADS);
                    auto bothSides = [&](const QVector3D& a, const QVector3D& b, const QVector3D& c, const QVector3D& d) {
                        // Emit each face with both windings so the handle block
                        // is visible regardless of which side of the bow the
                        // camera is on (GL_CULL_FACE is enabled in this view).
                        m.addQuad(a, b, c, d, BLACK);
                        m.addQuad(d, c, b, a, BLACK);
                    };
                    // Back face
                    bothSides(LBR, LBL, UBL, UBR);
                    // Belly face
                    bothSides(UbR, UbL, LbL, LbR);
                    // +z side
                    bothSides(LbR, LBR, UBR, UbR);
                    // -z side
                    bothSides(UbL, UBL, LBL, LbL);
                    // Upper end cap (joins limb cross-section)
                    bothSides(UBR, UBL, UbL, UbR);
                    // Lower end cap
                    bothSides(LbR, LbL, LBL, LBR);
                    handle = std::make_unique<Model>(m);
                }
            }
        }
        else {
            upper_right = nullptr;
            upper_left  = nullptr;
            lower_right = nullptr;
            lower_left  = nullptr;
            handle      = nullptr;
        }
    }

    update();
}

void LimbView::exportImage() {
    const char* PNG_FILE = "PNG image (*.png)";
    const char* JPG_FILE = "JPG image (*.jpg)";
    const char* BMP_FILE = "BMP image (*.bmp)";

    QFileDialog dialog(this);
    dialog.setAcceptMode(QFileDialog::AcceptSave);
    dialog.setNameFilters({PNG_FILE, JPG_FILE, BMP_FILE});
    dialog.selectFile("Export");

    // Todo: Is there a better way to connect default suffix to the selected name filter?
    // TODO: filterSelected is not triggered on some desktops (Linux/Cinnamon for example)
    QObject::connect(&dialog, &QFileDialog::filterSelected, [&](const QString &filter) {
        if(filter == PNG_FILE) {
            dialog.setDefaultSuffix(".png");
        }
        else if(filter == JPG_FILE) {
            dialog.setDefaultSuffix(".jpg");
        }
        else if(filter == BMP_FILE) {
            dialog.setDefaultSuffix(".bmp");
        }
    });

    dialog.selectNameFilter(PNG_FILE);
    emit dialog.filterSelected(PNG_FILE);

    if(dialog.exec() == QDialog::Accepted) {
        // Render widget to pixmap
        QPixmap pixmap(size());
        QPainter painter(&pixmap);
        render(&painter);

        // Save pixmap to selected file path
        QString path = dialog.selectedFiles().first();
        if(!pixmap.save(path)) {
            QMessageBox::critical(this, "Error", "Failed to export plot to " + path);
        }
    }
}

void LimbView::viewProfile() {
    rot_x = 0.0f;
    rot_y = 0.0f;
    viewFit();
}

void LimbView::viewTop() {
    rot_x = 90.0f;
    rot_y = 0.0f;
    viewFit();
}

void LimbView::view3D() {
    rot_x = DEFAULT_ROT_X;
    rot_y = DEFAULT_ROT_Y;
    viewFit();
}

void LimbView::viewSymmetric(bool checked) {
    // v5: no longer meaningful; left as a no-op for binary compatibility.
    (void)checked;
    update();
}

void LimbView::viewFit() {
    shift_x = 0.0f;
    shift_y = 0.0f;
    zoom = DEFAULT_ZOOM;
    update();
}

void LimbView::initializeGL() {
    initializeOpenGLFunctions();

    // OpenGL configuration

    glEnable(GL_DEPTH_TEST);
    glEnable(GL_CULL_FACE);
    glLineWidth(1.0f);

    // Shaders

    background_shader = new QOpenGLShaderProgram(this);
    background_shader->addShaderFromSourceFile(QOpenGLShader::Vertex, ":/shaders/BackgroundShader.vert");
    background_shader->addShaderFromSourceFile(QOpenGLShader::Fragment, ":/shaders/BackgroundShader.frag");
    background_shader->bindAttributeLocation("modelPosition", 0);
    background_shader->bindAttributeLocation("modelNormal", 1);
    background_shader->bindAttributeLocation("modelColor", 2);
    background_shader->link();

    model_shader = new QOpenGLShaderProgram(this);
    model_shader->addShaderFromSourceFile(QOpenGLShader::Vertex, ":/shaders/ModelShader.vert");
    model_shader->addShaderFromSourceFile(QOpenGLShader::Fragment, ":/shaders/ModelShader.frag");
    model_shader->bindAttributeLocation("modelPosition", 0);
    model_shader->bindAttributeLocation("modelNormal", 1);
    model_shader->bindAttributeLocation("modelColor", 2);
    model_shader->link();

    model_shader->bind();
    model_shader->setUniformValue("cameraPosition", CAMERA_POSITION);
    model_shader->setUniformValue("lightPosition", LIGHT_POSITION);
    model_shader->setUniformValue("lightColor", LIGHT_COLOR);
    model_shader->setUniformValue("ambientStrength", MATERIAL_AMBIENT_STRENGTH);
    model_shader->setUniformValue("diffuseStrength", MATERIAL_DIFFUSE_STRENGTH);
    model_shader->setUniformValue("specularStrength", MATERIAL_SPECULAR_STRENGTH);
    model_shader->setUniformValue("materialShininess", MATERIAL_SHININESS);
    model_shader->release();

    // Create background mesh

    Mesh background_mesh(GL_QUADS);
    background_mesh.addVertex({ 1.0f,  1.0f, 0.0f }, { 0.0f, 0.0f, 0.0f }, BACKGROUND_COLOR_1);
    background_mesh.addVertex({-1.0f,  1.0f, 0.0f }, { 0.0f, 0.0f, 0.0f }, BACKGROUND_COLOR_1);
    background_mesh.addVertex({-1.0f, -1.0f, 0.0f }, { 0.0f, 0.0f, 0.0f }, BACKGROUND_COLOR_2);
    background_mesh.addVertex({ 1.0f, -1.0f, 0.0f }, { 0.0f, 0.0f, 0.0f }, BACKGROUND_COLOR_2);

    QDate date = QDate::currentDate();
    if(date.month() == 12 && (date.day() == 24 || date.day() == 25 || date.day() == 26)) {
        auto create_star = [&](float x0, float y0, float r, float R, float alpha, unsigned n, const QColor& color) {
            float ratio = float(this->width())/float(this->height());
            float beta = 2.0*M_PI/n;
            float z0 = -0.1;
            for(unsigned i = 0; i < n; ++i) {
                float phi = alpha + i*beta;
                QVector3D p0(x0, y0, z0);
                QVector3D p1(x0 - r*sin(phi - beta/2), y0 + ratio*r*cos(phi - beta/2), z0);
                QVector3D p2(x0 - R*sin(phi), y0 + ratio*R*cos(phi), z0);
                QVector3D p3(x0 - r*sin(phi + beta/2), y0 + ratio*r*cos(phi + beta/2), z0);
                background_mesh.addQuad(p0, p1, p2, p3, color);
            }
        };

        srand(std::time(nullptr));
        auto random_in_range = [](float lower, float upper) {
            return lower + static_cast<float>(rand())/static_cast<float>(RAND_MAX/(upper - lower));
        };

        for(unsigned i = 0; i < 30; ++i) {
            float x = random_in_range(-1.0f, 1.0f);
            float y = random_in_range(-1.0f, 1.0f);
            float r = random_in_range(0.001f, 0.005f);
            create_star(x, y, r, 3.0*r, 0.0, 5, BACKGROUND_COLOR_2);
        }
    }

    background = std::make_unique<Model>(background_mesh);
}

void LimbView::paintGL() {
    // Draw background
    glClear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
    background->draw(background_shader);

    // Draw limbs, if available (v5: independent upper and lower meshes, each
    // already contains both lateral halves of its limb).
    if(upper_right != nullptr && lower_right != nullptr) {
        Bounds bounds = upper_right->getBounds();
        bounds.extend(lower_right->getBounds());
        if(handle != nullptr) {
            bounds.extend(handle->getBounds());
        }

        QMatrix4x4 m_model;
        m_model.setToIdentity();
        m_model.rotate(rot_x, 1.0f, 0.0f, 0.0f);
        m_model.rotate(rot_y, 0.0f, 1.0f, 0.0f);
        m_model.scale(1.0f/bounds.diagonal());
        m_model.translate(-bounds.center());

        QMatrix4x4 m_view;
        m_view.setToIdentity();
        m_view.lookAt(CAMERA_POSITION, {0.0f, 0.0f, 0.0f}, {0.0f, 1.0f, 0.0});

        float aspect_ratio = float(this->height())/this->width();
        QMatrix4x4 m_projection;
        m_projection.setToIdentity();
        m_projection.ortho(-0.5f*zoom + shift_x, 0.5f*zoom + shift_x, (-0.5f*zoom + shift_y)*aspect_ratio, ( 0.5f*zoom + shift_y)*aspect_ratio, 0.01f, 100.0f);

        model_shader->bind();
        model_shader->setUniformValue("modelMatrix", m_model);
        model_shader->setUniformValue("normalMatrix", m_model.normalMatrix());
        model_shader->setUniformValue("viewMatrix", m_view);
        model_shader->setUniformValue("projectionMatrix", m_projection);
        model_shader->release();

        upper_right->draw(model_shader);
        lower_right->draw(model_shader);
        if(handle != nullptr) {
            handle->draw(model_shader);
        }
    }
}

void LimbView::mousePressEvent(QMouseEvent *event) {
    mouse_pos = event->pos();
    QOpenGLWidget::mousePressEvent(event);
}

void LimbView::mouseMoveEvent(QMouseEvent *event) {
    int delta_x = event->position().x() - mouse_pos.x();
    int delta_y = event->position().y() - mouse_pos.y();

    if(event->buttons() & Qt::LeftButton)
    {
        rot_x += ROT_SPEED*delta_y;
        rot_y += ROT_SPEED*delta_x;
        update();
    }
    else if(event->buttons() & Qt::MiddleButton)
    {
        shift_x -= float(delta_x)/this->width()*zoom;
        shift_y += float(delta_y)/this->height()*zoom;
        update();
    }

    mouse_pos = event->pos();
}

void LimbView::wheelEvent(QWheelEvent* event) {
    float mouse_ratio_x = float(event->position().x())/this->width();
    float mouse_ratio_y = float(event->position().y())/this->height();
    float delta_zoom = -ZOOM_SPEED*event->angleDelta().y()/120.0f*zoom;    // Dividing by 120 gives the number of 15 degree steps on a standard mouse

    shift_x -= (mouse_ratio_x - 0.5f)*delta_zoom;
    shift_y += (mouse_ratio_y - 0.5f)*delta_zoom;
    zoom += delta_zoom;

    update();
}
